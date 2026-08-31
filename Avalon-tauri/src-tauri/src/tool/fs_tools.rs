// 基础文件/终端工具实现
//
// 5 个工具：read_file / write_file / delete_file / get_directory_contents 同步（std），
// run_shell_command 异步（tokio::process，带超时 + 进程树清理）。
// 工具签名统一：(args, &Sandbox) -> String，参数错误/越界/执行错误均以字符串返回。
//
// 所有路径先过 Sandbox::resolve，且用它返回的规范化路径去做实际 IO ——
// 拿原始字符串再做一次 open 等于把校验白做了。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::future;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

use super::sandbox::Sandbox;

/// 读文件上限：5MB，超限截断（防大文件塞爆上下文）
const MAX_READ: usize = 5 * 1024 * 1024;
/// 终端命令超时
const SHELL_TIMEOUT: Duration = Duration::from_secs(30);
/// 命令输出上限：stdout/stderr 各 64KB
const OUT_LIMIT: usize = 64 * 1024;
/// 单条命令的参数个数上限，纯粹防跑飞
const MAX_ARGS: usize = 64;

/// 取出字符串参数并过一遍沙箱。
///
/// 被拒时落 warn 并带上模型原本想访问的路径 —— 用户据此判断
/// 要不要把某个目录加进 workspace_roots，而不是只看到模型说「我没权限」
fn resolve_arg(args: &Value, key: &str, sb: &Sandbox) -> Result<PathBuf, String> {
    let Some(raw) = args.get(key).and_then(Value::as_str) else {
        return Err(format!("参数错误: 缺少 {key} 或类型应为字符串"));
    };
    sb.resolve(raw).map_err(|e| {
        log::warn!(target: "tool", "沙箱拦截文件访问: {raw}（{e}）");
        format!("参数错误: {e}")
    })
}

/// 读取指定文件内容（UTF-8 文本），超 5MB 截断
pub fn read_file(args: &Value, sb: &Sandbox) -> String {
    let file_path = match resolve_arg(args, "file_path", sb) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let file = match std::fs::File::open(&file_path) {
        Ok(f) => f,
        Err(e) => return format!("读取文件失败: {e}"),
    };

    // 多读 1 字节判断是否超限
    let mut buf = Vec::new();
    if let Err(e) = file.take((MAX_READ + 1) as u64).read_to_end(&mut buf) {
        return format!("读取文件失败: {e}");
    }
    let truncated = buf.len() > MAX_READ;
    if truncated {
        buf.truncate(MAX_READ);
    }

    match String::from_utf8(buf) {
        Ok(content) => {
            if truncated {
                format!("{content}\n...[文件过大，已截断前 5MB]")
            } else {
                content
            }
        }
        Err(_) => "读取文件失败: 文件不是有效的 UTF-8 文本（可能是二进制文件）".to_string(),
    }
}

/// 创建或覆盖写入文件，自动创建父目录
pub fn write_file(args: &Value, sb: &Sandbox) -> String {
    let file_path = match resolve_arg(args, "file_path", sb) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some(content) = args.get("content").and_then(Value::as_str) else {
        return "参数错误: 缺少 content 或类型应为字符串".to_string();
    };

    // 自动建父目录（省去一次 mkdir 工具调用）。
    // 安全性由上面的 resolve 保证：父目录是已校验路径的前缀，必然也在工作区内
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("写入文件失败: {e}");
            }
        }
    }

    match std::fs::write(&file_path, content) {
        Ok(_) => format!("文件 {} 写入成功", file_path.display()),
        Err(e) => format!("写入文件失败: {e}"),
    }
}

/// 删除指定文件（仅文件，删目录报错）
pub fn delete_file(args: &Value, sb: &Sandbox) -> String {
    let file_path = match resolve_arg(args, "file_path", sb) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match std::fs::remove_file(&file_path) {
        Ok(_) => format!("文件 {} 已删除", file_path.display()),
        Err(e) => format!("删除文件失败: {e}"),
    }
}

/// 获取目录下文件与子目录，目录在前文件在后、各自按名排序
pub fn get_directory_contents(args: &Value, sb: &Sandbox) -> String {
    let dir_path = match resolve_arg(args, "directory_path", sb) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let entries = match std::fs::read_dir(&dir_path) {
        Ok(e) => e,
        Err(e) => return format!("获取目录内容失败: {e}"),
    };

    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => dirs.push(name),
            _ => files.push(name),
        }
    }
    dirs.sort();
    files.sort();

    let mut out = String::new();
    for d in &dirs {
        out.push_str(&format!("[D] {d}\n"));
    }
    for f in &files {
        out.push_str(&format!("[F] {f}\n"));
    }

    if out.is_empty() {
        "目录为空".to_string()
    } else {
        out.trim_end().to_string()
    }
}

/// 在终端执行白名单内的命令，返回 stdout+stderr，超时 30s 终止。
///
/// 不经过 cmd /C / sh -c —— 直接 spawn 可执行文件，argv 逐个传递。
/// 这是白名单能成立的前提：交给 shell 解释的话，检查「第一个词是不是 ping」
/// 挡不住 `ping x && del /s /q C:\`，而 && | > ; 在这里没有任何特殊含义。
/// 副作用是管道与重定向也一并没了，这是设计取舍不是缺陷。
pub async fn run_shell_command(args: &Value, sb: &Sandbox) -> String {
    let Some(command) = args.get("command").and_then(Value::as_str) else {
        return "参数错误: 缺少 command 或类型应为字符串".to_string();
    };

    let exe = match sb.resolve_command(command) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(target: "tool", "沙箱拦截终端命令: {command}（{e}）");
            return format!("参数错误: {e}");
        }
    };

    let argv = match parse_args(args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let mut cmd = Command::new(&exe);
    cmd.args(&argv)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // stdin 给空管道而不是继承：交互式命令（等确认的 ping -t、要密码的工具）
        // 否则会一直等一个永远不会来的输入，白白耗满 30 秒超时
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    // cwd 落在工作区，让命令里的相对路径有个确定的落点。
    // 这不构成安全边界 —— 边界是白名单本身，cwd 只影响子进程自己怎么解释相对路径
    if let Some(dir) = sb.work_dir() {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return format!("执行命令失败: {e}"),
    };
    let pid = child.id();

    match tokio::time::timeout(SHELL_TIMEOUT, collect_output(&mut child)).await {
        Ok(Ok((stdout, stderr, status))) => {
            let code = status.code().unwrap_or(-1);
            let mut out = decode_output(&stdout);
            let err = decode_output(&stderr);
            if !out.is_empty() && !err.is_empty() {
                out.push('\n');
            }
            out.push_str(&err);
            if code != 0 {
                out.push_str(&format!("\n[退出码: {code}]"));
            }
            out
        }
        Ok(Err(e)) => format!("执行命令失败: {e}"),
        Err(_) => {
            kill_tree(pid);
            format!("执行命令超时（{} 秒），已终止", SHELL_TIMEOUT.as_secs())
        }
    }
}

/// 解析 args 数组。缺省视为无参数，非数组或含非字符串项则报错。
///
/// 刻意不接受「一个大字符串再由我们切分」：带空格的路径、引号嵌套怎么切
/// 全是坑，而切错的后果是参数错位。要数组就没有猜的余地
fn parse_args(args: &Value) -> Result<Vec<String>, String> {
    let Some(list) = args.get("args") else {
        return Ok(Vec::new());
    };
    if list.is_null() {
        return Ok(Vec::new());
    }
    let Some(list) = list.as_array() else {
        return Err(
            "参数错误: args 必须是字符串数组，例如 [\"-n\", \"1\", \"127.0.0.1\"]".to_string(),
        );
    };
    if list.len() > MAX_ARGS {
        return Err(format!("参数错误: args 最多 {MAX_ARGS} 项"));
    }
    list.iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                format!("参数错误: args 的每一项都必须是字符串（收到 {v}），数字也要加引号")
            })
        })
        .collect()
}

/// 并发读 stdout/stderr（各限 OUT_LIMIT）并等待进程退出
async fn collect_output(
    child: &mut Child,
) -> std::io::Result<(Vec<u8>, Vec<u8>, std::process::ExitStatus)> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("stdout 未捕获"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("stderr 未捕获"))?;

    let (out, err, status) = future::join3(
        read_limited(stdout, OUT_LIMIT),
        read_limited(stderr, OUT_LIMIT),
        child.wait(),
    )
    .await;

    Ok((out?, err?, status?))
}

/// 读最多 limit 字节
async fn read_limited<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(limit.min(8192));
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await? {
            0 => break,
            n => {
                if buf.len() + n > limit {
                    buf.extend_from_slice(&chunk[..limit - buf.len()]);
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }
    Ok(buf)
}

/// 超时后杀进程树（Windows taskkill /T /F，Unix kill）
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
}

/// 命令输出解码：优先 UTF-8，Windows 上回退 GBK（cmd 默认代码页）
fn decode_output(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        let (cow, _, _) = encoding_rs::GBK.decode(bytes);
        return cow.into_owned();
    }
    #[cfg(not(target_os = "windows"))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("avalon_fs_test/{tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sb(tag: &str) -> Sandbox {
        Sandbox::with(vec![workspace(tag)], vec!["hostname", "ping"])
    }

    #[test]
    fn 工作区内可以写读删() {
        let sb = sb("rw");
        let p = workspace("rw").join("子目录/笔记.txt");
        let p = p.to_str().unwrap();

        let out = write_file(&json!({"file_path": p, "content": "内容"}), &sb);
        assert!(out.contains("写入成功"), "实际: {out}");
        assert_eq!(read_file(&json!({"file_path": p}), &sb), "内容");
        assert!(delete_file(&json!({"file_path": p}), &sb).contains("已删除"));
    }

    #[test]
    fn 越界读取被拒且不落到磁盘操作() {
        // 拒绝必须发生在 open 之前，错误里不能出现「系统找不到文件」这类 IO 措辞
        let out = read_file(&json!({"file_path": "../../Avalon-config.toml"}), &sb("deny"));
        assert!(out.starts_with("参数错误"), "实际: {out}");
    }

    #[test]
    fn 越界写入不会创建父目录() {
        let outside = std::env::temp_dir().join("avalon_fs_test_越界/x.txt");
        let out = write_file(
            &json!({"file_path": outside.to_str().unwrap(), "content": "x"}),
            &sb("deny2"),
        );
        assert!(out.starts_with("参数错误"), "实际: {out}");
        // write_file 会自动建父目录，校验若放在建目录之后就会留下痕迹
        assert!(!outside.parent().unwrap().exists(), "越界路径的父目录不该被创建");
    }

    #[test]
    fn args缺省与类型校验() {
        assert_eq!(parse_args(&json!({"command": "hostname"})).unwrap().len(), 0);
        assert_eq!(parse_args(&json!({"args": null})).unwrap().len(), 0);
        assert_eq!(parse_args(&json!({"args": ["-n", "1"]})).unwrap().len(), 2);
        // 数字项要明确报错并说明怎么改，不能静默转成字符串
        let err = parse_args(&json!({"args": ["-n", 1]})).unwrap_err();
        assert!(err.contains("加引号"), "实际: {err}");
        let err = parse_args(&json!({"args": "-n 1"})).unwrap_err();
        assert!(err.contains("字符串数组"), "实际: {err}");
    }

    #[tokio::test]
    async fn 白名单命令能正常执行() {
        let out = run_shell_command(&json!({"command": "hostname"}), &sb("run")).await;
        assert!(!out.is_empty());
        assert!(!out.starts_with("参数错误"), "实际: {out}");
        assert!(!out.starts_with("执行命令失败"), "实际: {out}");
    }

    /// 这条是整个终端改造的核心断言。
    /// 走 cmd /C 的话 `>` 会被解释成重定向、凭空造出一个文件；
    /// 直接 spawn 则只是把 ">" 当成 hostname 的一个普通参数
    #[tokio::test]
    async fn 重定向符号不再有特殊含义() {
        let ws = workspace("meta");
        let target = ws.join("被重定向出来的文件.txt");
        let _ = std::fs::remove_file(&target);

        run_shell_command(
            &json!({"command": "hostname", "args": [">", target.to_str().unwrap()]}),
            &sb("meta"),
        )
        .await;

        assert!(!target.exists(), "> 被当成了重定向，shell 逃逸没堵住");
    }

    #[tokio::test]
    async fn 白名单外的命令被拒() {
        let out = run_shell_command(&json!({"command": "curl"}), &sb("deny3")).await;
        assert!(out.starts_with("参数错误"), "实际: {out}");
        assert!(out.contains("不在白名单"), "实际: {out}");
    }

    #[tokio::test]
    async fn 旧形态整串命令给出迁移提示() {
        let out = run_shell_command(&json!({"command": "ping -n 1 127.0.0.1"}), &sb("legacy")).await;
        assert!(out.contains("args"), "实际: {out}");
    }
}
