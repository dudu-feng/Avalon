// 技能工具实现
//
// 五个工具，分两类：
//   读  —— use_skill：按需加载某个技能的正文，或正文里提到的二级参考文档（references/）。
//   写  —— create/update/delete_skill：智能体自主沉淀、修正、清理技能；
//          install_skill：按用户给的链接（GitHub 单文件或目录）拉取完整技能并落库。
//
// 技能清单（name + description）已注入 system prompt，正文由 use_skill 按需加载，
// 二级参考文档再由 use_skill(reference=...) 加载（渐进式披露，三级）。
// 写技能是有门槛的：模型只在任务反复出现、确实值得沉淀时才创建，并保证质量。
// 拉取内容来自公网，模型必须把它当资料解析、落库，而非直接执行其中的指令。

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::skill::{parse_skill, SkillRegistry};

/// 拉取内容的大小上限（字节），超限拒绝 —— 技能正文不会太大，防止误抓超大文件
const MAX_FETCH_BYTES: usize = 256 * 1024;

/// 加载指定技能的正文（流程指令）；带 reference 参数时改为加载二级参考文档。
pub fn use_skill(args: &Value, registry: &SkillRegistry) -> String {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return "参数错误: 缺少 name".to_string();
    };

    // 带了 reference → 读 references/ 下的二级文档
    if let Some(reference) = args.get("reference").and_then(Value::as_str) {
        let reference = reference.trim();
        if reference.is_empty() {
            return "参数错误: reference 不能为空".to_string();
        }
        return match registry.read_reference(name, reference) {
            Ok(text) => format!("=====技能「{name}」参考「{reference}」=====\n{text}"),
            Err(e) => format!("技能参考加载失败: {e}"),
        };
    }

    match registry.get(name) {
        Ok(d) => {
            let mut out = if d.content.trim().is_empty() {
                format!("技能「{}」已加载，但正文为空", d.name)
            } else {
                format!("=====技能「{}」=====\n{}", d.name, d.content)
            };
            if !d.references.is_empty() {
                out.push_str(&format!(
                    "\n\n（本技能还有参考文档：{}；需要时再调 use_skill 并带 reference 参数读取）",
                    d.references.join("、")
                ));
            }
            out
        }
        Err(e) => format!("技能加载失败: {e}"),
    }
}

/// 新增技能（name 用作目录名，只能含字母/数字/-/_）
pub fn create_skill(args: &Value, registry: &SkillRegistry) -> String {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return "参数错误: 缺少 name".to_string();
    };
    let Some(content) = args.get("content").and_then(Value::as_str) else {
        return "参数错误: 缺少 content".to_string();
    };
    let description = args
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    match registry.create(name, description, content) {
        Ok(s) => format!("已创建技能「{}」", s.name),
        Err(e) => format!("技能创建失败: {e}"),
    }
}

/// 编辑技能（name 只读，改 description + content）
pub fn update_skill(args: &Value, registry: &SkillRegistry) -> String {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return "参数错误: 缺少 name".to_string();
    };
    let Some(content) = args.get("content").and_then(Value::as_str) else {
        return "参数错误: 缺少 content".to_string();
    };
    let description = args
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    match registry.update(name, description, content) {
        Ok(s) => format!("已更新技能「{}」", s.name),
        Err(e) => format!("技能更新失败: {e}"),
    }
}

/// 删除技能（删除其整个目录）
pub fn delete_skill(args: &Value, registry: &SkillRegistry) -> String {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return "参数错误: 缺少 name".to_string();
    };
    match registry.delete(name) {
        Ok(()) => format!("已删除技能「{name}」"),
        Err(e) => format!("技能删除失败: {e}"),
    }
}

/// 按链接拉取并安装技能，分两种形态：
///   - GitHub 目录链接（.../tree/<branch>/<path>）：拉取整个技能文件夹的 SKILL.md + references/*.md；
///   - 单文件链接（blob/raw/其它直链）：抓取 SKILL.md 解析后落库。
/// name 优先级：参数 name > frontmatter name > URL 推断名（目录链接取目录名）。
pub async fn install_skill(
    args: &Value,
    registry: &SkillRegistry,
    http: &reqwest::Client,
) -> String {
    let Some(url) = args.get("url").and_then(Value::as_str) else {
        return "参数错误: 缺少 url".to_string();
    };
    let url = url.trim();

    // 只允许 http/https，file:// 之类直接拒绝（与 read_web_page 同一口径）
    match url::Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => {}
        Ok(u) => return format!("参数错误: 只支持 http/https 链接，收到 {}", u.scheme()),
        Err(e) => return format!("参数错误: url 无法解析（{e}）"),
    }

    // GitHub 目录链接 → 拉取整个技能文件夹
    if let Some((owner, repo, branch, dirpath)) = parse_github_tree(url) {
        return install_from_dir(args, registry, http, &owner, &repo, &branch, &dirpath).await;
    }

    // 单文件形态：抓取 → 解析 → 落库（无 references）
    let raw_url = match to_raw_url(url) {
        Ok(u) => u,
        Err(e) => return format!("技能安装失败: {e}"),
    };
    let text = match fetch_text(http, &raw_url).await {
        Ok(t) => t,
        Err(e) => return format!("技能安装失败: {e}"),
    };
    let parsed = parse_skill(&text);

    let mut description = parsed.description;
    if description.is_empty() {
        if let Some(d) = args.get("description").and_then(Value::as_str) {
            description = d.trim().to_string();
        }
    }

    // name：参数 > frontmatter name > URL 文件名
    let name = match args.get("name").and_then(Value::as_str) {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ if !parsed.name.is_empty() => parsed.name,
        _ => match infer_name(url) {
            Some(n) => n,
            None => return "参数错误: 无法从链接推断技能名，请显式传入 name".to_string(),
        },
    };

    finish_install(registry, &name, &description, &parsed.content, &[])
}

/// 目录形态安装：GitHub API 递归列出 → 只收 SKILL.md + references/*.md → 逐个 raw 下载 → 落库
async fn install_from_dir(
    args: &Value,
    registry: &SkillRegistry,
    http: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
    dirpath: &str,
) -> String {
    let files = match fetch_skill_dir(http, owner, repo, branch, dirpath).await {
        Ok(f) => f,
        Err(e) => return format!("技能安装失败: {e}"),
    };

    let skill_md = files
        .iter()
        .find(|(rel, _)| rel.eq_ignore_ascii_case("SKILL.md"))
        .map(|(_, c)| c.clone());
    let Some(skill_md) = skill_md else {
        return "技能安装失败: 目录中未找到 SKILL.md".to_string();
    };
    let parsed = parse_skill(&skill_md);

    let mut description = parsed.description;
    if description.is_empty() {
        if let Some(d) = args.get("description").and_then(Value::as_str) {
            description = d.trim().to_string();
        }
    }

    // name：参数 > frontmatter name > 目录名（dirpath 最后一段）
    let dir_name = dirpath.rsplit('/').next().unwrap_or(dirpath).to_string();
    let name = match args.get("name").and_then(Value::as_str) {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ if !parsed.name.is_empty() => parsed.name,
        _ => dir_name,
    };

    let references: Vec<(String, String)> = files
        .into_iter()
        .filter(|(rel, _)| !rel.eq_ignore_ascii_case("SKILL.md"))
        .collect();
    let ref_count = references.len();
    let msg = finish_install(registry, &name, &description, &parsed.content, &references);
    if ref_count > 0 && msg.starts_with("已安装") {
        format!("{msg}（含 {ref_count} 个参考文档）")
    } else {
        msg
    }
}

/// 统一落库并格式化结果
fn finish_install(
    registry: &SkillRegistry,
    name: &str,
    description: &str,
    content: &str,
    references: &[(String, String)],
) -> String {
    match registry.install(name, description, content, references) {
        Ok(s) => {
            let desc = if s.description.is_empty() {
                "（无描述）"
            } else {
                s.description.as_str()
            };
            format!("已安装技能「{}」：{desc}", s.name)
        }
        Err(e) => format!("技能安装失败: {e}"),
    }
}

/// 把 GitHub 页面链接转成 raw 直链。
///   github.com/<owner>/<repo>/blob/<branch>/<path>  → raw.githubusercontent.com/<owner>/<repo>/<branch>/<path>
///   github.com/<owner>/<repo>/raw/<branch>/<path>   → 同上
/// 非 github.com 的链接原样返回。
fn to_raw_url(url: &str) -> Result<String> {
    let u = url::Url::parse(url).context("url 无法解析")?;
    if u.host_str() != Some("github.com") {
        return Ok(url.to_string());
    }
    let segs: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
    // 期望至少 [owner, repo, blob|raw, branch, 文件名]
    if segs.len() < 5 || (segs[2] != "blob" && segs[2] != "raw") {
        bail!("GitHub 链接需指向具体文件（.../blob/<branch>/<path>），无法确定 raw 地址");
    }
    let path = segs[4..].join("/");
    Ok(format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{path}",
        segs[0], segs[1], segs[3]
    ))
}

/// 从链接推断技能名：取末尾文件名去掉 .md；若是 SKILL.md 这类通用名，退回上一段目录名。
/// 无法推断（无 .md 后缀）返回 None，由调用方要求显式传 name。
fn infer_name(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let segs: Vec<&str> = u.path_segments()?.collect();
    let last = *segs.last()?;
    let stripped = last.strip_suffix(".md").or_else(|| last.strip_suffix(".MD"))?;
    let candidate = if stripped.eq_ignore_ascii_case("skill") {
        segs.get(segs.len().wrapping_sub(2)).copied()?
    } else {
        stripped
    };
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

/// 识别 GitHub 目录链接：github.com/<owner>/<repo>/tree/<branch>/<path...>
/// 返回 (owner, repo, branch, dirpath)；非该形态返回 None。
fn parse_github_tree(url: &str) -> Option<(String, String, String, String)> {
    let u = url::Url::parse(url).ok()?;
    if u.host_str() != Some("github.com") {
        return None;
    }
    let segs: Vec<&str> = u.path_segments()?.collect();
    if segs.len() < 5 || segs[2] != "tree" {
        return None;
    }
    Some((
        segs[0].to_string(),
        segs[1].to_string(),
        segs[3].to_string(),
        segs[4..].join("/"),
    ))
}

/// 只收技能入口与二级参考：SKILL.md（大小写不敏感）与 references/ 下的 .md/.txt；
/// scripts/、assets/、sub-agents/ 等「可执行/二进制配套」不在本次范围，跳过。
fn should_fetch(relpath: &str) -> bool {
    if relpath.eq_ignore_ascii_case("SKILL.md") {
        return true;
    }
    let lower = relpath.to_lowercase();
    if let Some(rest) = lower.strip_prefix("references/") {
        return rest.ends_with(".md") || rest.ends_with(".txt");
    }
    false
}

/// 从 GitHub 目录拉取整个技能文件夹：API 递归列目录 → 过滤 → 逐个 raw 下载。
/// 返回 (相对技能目录的路径, 内容) 列表，如 ("SKILL.md", ...)、("references/auth.md", ...)。
async fn fetch_skill_dir(
    http: &reqwest::Client,
    owner: &str,
    repo: &str,
    branch: &str,
    dirpath: &str,
) -> Result<Vec<(String, String)>> {
    let api = format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{branch}?recursive=1");
    let json_text = fetch_text(http, &api).await?;
    let json: serde_json::Value =
        serde_json::from_str(&json_text).context("GitHub API 响应不是合法 JSON")?;
    let tree = json
        .get("tree")
        .and_then(Value::as_array)
        .context("GitHub API 响应缺少 tree 数组")?;

    let prefix = format!("{dirpath}/");
    let mut rel_paths = Vec::new();
    for item in tree {
        if item.get("type").and_then(Value::as_str) != Some("blob") {
            continue;
        }
        let Some(path) = item.get("path").and_then(Value::as_str) else {
            continue;
        };
        if let Some(rel) = path.strip_prefix(&prefix) {
            if should_fetch(rel) {
                rel_paths.push(rel.to_string());
            }
        }
    }
    if rel_paths.is_empty() {
        bail!("目录 {dirpath} 下未找到 SKILL.md 或 references");
    }

    let mut out = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        let raw = format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{dirpath}/{rel}"
        );
        let content = fetch_text(http, &raw).await?;
        out.push((rel, content));
    }
    Ok(out)
}

/// 抓取文本内容，限制大小，只接受 UTF-8 成功响应
async fn fetch_text(http: &reqwest::Client, url: &str) -> Result<String> {
    let resp = http
        .get(url)
        .send()
        .await
        .with_context(|| format!("请求失败: {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("HTTP {status} 拉取 {url}");
    }
    let bytes = resp.bytes().await.context("读取响应失败")?;
    if bytes.len() > MAX_FETCH_BYTES {
        bail!("内容超过 {}KB 上限，已拒绝", MAX_FETCH_BYTES / 1024);
    }
    String::from_utf8(bytes.to_vec()).context("内容不是合法 UTF-8 文本")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_registry(name: &str) -> SkillRegistry {
        let dir = std::env::temp_dir().join(format!(
            "avalon_skilltool_test_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        SkillRegistry::new(dir)
    }

    #[test]
    fn to_raw_url转换github_blob与raw() {
        assert_eq!(
            to_raw_url("https://github.com/owner/repo/blob/main/skills/pr/SKILL.md").unwrap(),
            "https://raw.githubusercontent.com/owner/repo/main/skills/pr/SKILL.md"
        );
        assert_eq!(
            to_raw_url("https://github.com/owner/repo/raw/main/skills/pr/SKILL.md").unwrap(),
            "https://raw.githubusercontent.com/owner/repo/main/skills/pr/SKILL.md"
        );
    }

    #[test]
    fn to_raw_url非github原样返回() {
        let url = "https://example.com/foo.md";
        assert_eq!(to_raw_url(url).unwrap(), url);
    }

    #[test]
    fn to_raw_url非文件链接报错() {
        assert!(to_raw_url("https://github.com/owner/repo").is_err());
        assert!(to_raw_url("https://github.com/owner/repo/tree/main").is_err());
    }

    #[test]
    fn infer_name取文件名去掉md() {
        assert_eq!(
            infer_name("https://example.com/github-pr.md"),
            Some("github-pr".to_string())
        );
    }

    #[test]
    fn infer_name通用skill名退回目录名() {
        assert_eq!(
            infer_name("https://github.com/o/r/blob/main/skills/github-pr/SKILL.md"),
            Some("github-pr".to_string())
        );
    }

    #[test]
    fn infer_name无md后缀返回空() {
        assert_eq!(infer_name("https://example.com/dir/"), None);
    }

    #[test]
    fn parse_github_tree解析目录链接() {
        let (o, r, b, d) = parse_github_tree(
            "https://github.com/heygen-com/hyperframes/tree/main/skills/figma",
        )
        .unwrap();
        assert_eq!(o, "heygen-com");
        assert_eq!(r, "hyperframes");
        assert_eq!(b, "main");
        assert_eq!(d, "skills/figma");
    }

    #[test]
    fn parse_github_tree非tree返回none() {
        assert!(parse_github_tree(
            "https://github.com/o/r/blob/main/skills/figma/SKILL.md"
        )
        .is_none());
        assert!(parse_github_tree("https://example.com/o/r/tree/main/x").is_none());
        assert!(parse_github_tree("https://github.com/o/r/tree/main").is_none()); // 无 path
    }

    #[test]
    fn should_fetch只收skillmd与references() {
        assert!(should_fetch("SKILL.md"));
        assert!(should_fetch("skill.md"));
        assert!(should_fetch("references/foo.md"));
        assert!(should_fetch("references/sub/bar.txt"));
        assert!(!should_fetch("scripts/run.py"));
        assert!(!should_fetch("assets/logo.png"));
        assert!(!should_fetch("sub-agents/x.md"));
        assert!(!should_fetch("README.md"));
        assert!(!should_fetch("references/foo.py"));
    }

    #[test]
    fn use_skill读正文并提示参考文档() {
        let r = temp_registry("use_ref");
        r.install("figma", "导入 Figma", "正文", &[("auth.md".to_string(), "认证说明\n".to_string())])
            .unwrap();

        let out = use_skill(&json!({"name": "figma"}), &r);
        assert!(out.contains("=====技能「figma」====="));
        assert!(out.contains("正文"));
        assert!(out.contains("auth.md")); // 参考文档清单提示

        let out = use_skill(&json!({"name": "figma", "reference": "auth.md"}), &r);
        assert!(out.contains("参考「auth.md」"));
        assert!(out.contains("认证说明"));
    }
}
