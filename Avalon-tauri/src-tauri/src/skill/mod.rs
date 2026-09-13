// 技能注册表模块
//
// 管理智能体的「技能」（Skill）——可复用的流程知识包，按需加载，不常驻 system prompt。
// 与「灵魂」（soul，身份/长期设定，常驻）互补：灵魂回答「我是谁」，技能回答「某类任务怎么做」。
//
// 存储结构（每个技能一个子目录，对齐主流 SKILL.md 规范）：
//   data/skills/<name>/
//     SKILL.md          —— 入口：frontmatter（name + description）+ 正文
//     references/*.md   —— 可选的二级参考文档，正文里引用、按需加载
//
// 设计要点：
//   - name 取目录名作为唯一标识；frontmatter 的 name 字段在安装时作为命名依据。
//   - 渐进式披露（两级）：system prompt 只注入「name + description」清单（一句话/个）；
//     正文由 use_skill 加载；正文里提到的参考文档由 use_skill(reference=...) 再按需加载。
//   - 技能由 agent 按需使用，也可自主沉淀（create/update/delete）或从链接安装（install）。

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

/// 技能索引项（SKILL.md 的 frontmatter，不含正文）
#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
}

/// 单条技能 + 正文 + 参考文档列表（get 接口返回，供前端详情、agent 的 use_skill）
#[derive(Debug, Clone, Serialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub content: String,
    /// references/ 下的相对路径列表（如 "foo.md"、"sub/bar.md"），按需加载
    pub references: Vec<String>,
}

/// parse_skill 的解析结果（frontmatter 的 name/description + 正文）
#[derive(Debug, Clone)]
pub struct ParsedSkill {
    pub name: String,
    pub description: String,
    pub content: String,
}

/// 技能注册表：扫描 data/skills 目录，读写各技能的 SKILL.md 与 references/。
/// 文件少，不缓存；写操作串行化（Mutex），读不锁（单文件写不涉及跨文件一致性）。
pub struct SkillRegistry {
    skills_dir: PathBuf,
    write_lock: std::sync::Mutex<()>,
}

impl SkillRegistry {
    /// 从技能目录构造（data_root/skills）
    pub fn new(skills_dir: PathBuf) -> Self {
        Self {
            skills_dir,
            write_lock: std::sync::Mutex::new(()),
        }
    }

    /// 列出全部技能（name + description，按 name 排序）；目录不存在返回空列表
    pub fn list(&self) -> Result<Vec<Skill>> {
        if !self.skills_dir.exists() {
            return Ok(Vec::new());
        }
        let mut skills = Vec::new();
        let read = std::fs::read_dir(&self.skills_dir)
            .with_context(|| format!("读取技能目录失败: {}", self.skills_dir.display()))?;
        for entry in read {
            let entry = entry.with_context(|| format!("遍历技能目录失败: {}", self.skills_dir.display()))?;
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let md_path = entry.path().join("SKILL.md");
            // 缺 SKILL.md 或读失败的目录跳过（视为非技能目录）
            let Ok(text) = std::fs::read_to_string(&md_path) else {
                continue;
            };
            let parsed = parse_skill(&text);
            skills.push(Skill { name, description: parsed.description });
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
    }

    /// 读取单条技能 + 正文 + 参考文档列表
    pub fn get(&self, name: &str) -> Result<SkillDetail> {
        let name = validate_name(name)?;
        let md_path = self.skills_dir.join(&name).join("SKILL.md");
        let text = std::fs::read_to_string(&md_path)
            .with_context(|| format!("读取技能失败: {}", md_path.display()))?;
        let parsed = parse_skill(&text);
        let references = self.list_references(&name)?;
        Ok(SkillDetail {
            name,
            description: parsed.description,
            content: parsed.content,
            references,
        })
    }

    /// 列出某技能的参考文档相对路径（references/ 下的 .md/.txt，递归，按名排序）
    pub fn list_references(&self, name: &str) -> Result<Vec<String>> {
        let name = validate_name(name)?;
        self.scan_references(&name)
    }

    /// 读取某技能的参考文档正文（ref_path 相对 references/ 目录，如 "sub/foo.md"）
    pub fn read_reference(&self, name: &str, ref_path: &str) -> Result<String> {
        let name = validate_name(name)?;
        let rel = validate_ref_path(ref_path)?;
        let p = self.skills_dir.join(&name).join("references").join(&rel);
        std::fs::read_to_string(&p)
            .with_context(|| format!("读取技能参考失败: {}", p.display()))
    }

    /// 新增技能（写 data/skills/<name>/SKILL.md）
    pub fn create(&self, name: &str, description: &str, content: &str) -> Result<Skill> {
        let _guard = self.write_lock.lock().unwrap();
        let name = validate_name(name)?;
        let dir = self.skills_dir.join(&name);
        if dir.join("SKILL.md").exists() {
            return Err(anyhow!("技能 '{name}' 已存在"));
        }
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("创建技能目录失败: {}", dir.display()))?;
        std::fs::write(dir.join("SKILL.md"), render_skill_md(&name, description, content))
            .with_context(|| format!("写入技能失败: {}", dir.join("SKILL.md").display()))?;
        Ok(Skill {
            name,
            description: description.trim().to_string(),
        })
    }

    /// 更新技能（name 只读，改 description + content）
    pub fn update(&self, name: &str, description: &str, content: &str) -> Result<Skill> {
        let _guard = self.write_lock.lock().unwrap();
        let name = validate_name(name)?;
        let md_path = self.skills_dir.join(&name).join("SKILL.md");
        if !md_path.exists() {
            return Err(anyhow!("技能 '{name}' 不存在"));
        }
        std::fs::write(&md_path, render_skill_md(&name, description, content))
            .with_context(|| format!("写入技能失败: {}", md_path.display()))?;
        Ok(Skill {
            name,
            description: description.trim().to_string(),
        })
    }

    /// 安装技能（写 SKILL.md + references/*.md），供 install_skill 从外部拉取后落库。
    /// references 为 (相对路径, 正文) 列表，相对 references/ 目录；路径逐个校验防穿越。
    pub fn install(
        &self,
        name: &str,
        description: &str,
        content: &str,
        references: &[(String, String)],
    ) -> Result<Skill> {
        let _guard = self.write_lock.lock().unwrap();
        let name = validate_name(name)?;
        let dir = self.skills_dir.join(&name);
        if dir.join("SKILL.md").exists() {
            return Err(anyhow!("技能 '{name}' 已存在"));
        }
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("创建技能目录失败: {}", dir.display()))?;
        std::fs::write(dir.join("SKILL.md"), render_skill_md(&name, description, content))
            .with_context(|| format!("写入技能失败: {}", dir.join("SKILL.md").display()))?;

        if !references.is_empty() {
            let ref_dir = dir.join("references");
            for (rel, text) in references {
                let rel = validate_ref_path(rel)?;
                let target = ref_dir.join(&rel);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("创建参考目录失败: {}", parent.display()))?;
                }
                std::fs::write(&target, text)
                    .with_context(|| format!("写入参考文档失败: {}", target.display()))?;
            }
        }
        Ok(Skill {
            name,
            description: description.trim().to_string(),
        })
    }

    /// 删除技能（删除整个 data/skills/<name> 目录）
    pub fn delete(&self, name: &str) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap();
        let name = validate_name(name)?;
        let dir = self.skills_dir.join(&name);
        if !dir.exists() {
            return Err(anyhow!("技能 '{name}' 不存在"));
        }
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("删除技能目录失败: {}", dir.display()))?;
        Ok(())
    }

    /// 扫描 references/ 下的文本文件（.md/.txt），返回相对路径，递归、排序
    fn scan_references(&self, name: &str) -> Result<Vec<String>> {
        let ref_dir = self.skills_dir.join(name).join("references");
        if !ref_dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        walk_references(&ref_dir, "", &mut out)?;
        out.sort();
        Ok(out)
    }
}

/// 校验技能名（用作目录名，需防路径穿越与非法字符）。返回 trim 后的名称。
fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("技能名称不能为空"));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("技能名称只能包含字母、数字、- 或 _"));
    }
    Ok(name.to_string())
}

/// 校验参考文档相对路径（相对 references/ 目录），防路径穿越。返回 trim 后的路径。
fn validate_ref_path(path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(anyhow!("参考文档路径不能为空"));
    }
    let norm = Path::new(path);
    for comp in norm.components() {
        match comp {
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(anyhow!("参考文档路径非法（不能含 .. 或绝对路径）"));
            }
            _ => {}
        }
    }
    Ok(path.to_string())
}

/// 递归遍历 references 目录，收集 .md/.txt 相对路径
fn walk_references(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("读取参考目录失败: {}", dir.display()))?;
    for entry in read {
        let entry = entry.with_context(|| format!("遍历参考目录失败: {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            walk_references(&entry.path(), &rel, out)?;
        } else if ft.is_file() && is_text_ref(&name) {
            out.push(rel);
        }
    }
    Ok(())
}

fn is_text_ref(name: &str) -> bool {
    name.ends_with(".md") || name.ends_with(".txt")
}

/// 生成 SKILL.md 内容（frontmatter name + description + 正文）
fn render_skill_md(name: &str, description: &str, content: &str) -> String {
    format!(
        "---\nname: {}\ndescription: {}\n---\n\n{}",
        name.trim(),
        description.trim(),
        content.trim()
    )
}

/// 解析 SKILL.md：返回 frontmatter 的 name/description + 正文。
/// 首行是 `---` 则解析 frontmatter 到第二个 `---`；其余全部作为正文。
/// 无 frontmatter 时 name/description 为空、整篇当正文。
pub fn parse_skill(text: &str) -> ParsedSkill {
    let mut name = String::new();
    let mut description = String::new();
    let mut body = String::new();
    let mut lines = text.lines().peekable();

    if lines.peek().map(|l| l.trim() == "---").unwrap_or(false) {
        lines.next(); // 跳过开头的 ---
        for line in lines.by_ref() {
            let t = line.trim();
            if t == "---" {
                break;
            }
            if let Some(v) = t
                .strip_prefix("name:")
                .or_else(|| t.strip_prefix("name："))
            {
                name = v.trim().to_string();
            }
            if let Some(v) = t
                .strip_prefix("description:")
                .or_else(|| t.strip_prefix("description："))
            {
                description = v.trim().to_string();
            }
        }
    }

    // 跳过 frontmatter 与正文之间（以及正文开头）的空行，避免读出的正文带前导换行
    let mut body_started = false;
    for line in lines {
        if !body_started && line.trim().is_empty() {
            continue;
        }
        body_started = true;
        body.push_str(line);
        body.push('\n');
    }
    ParsedSkill {
        name,
        description,
        content: body.trim_end().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_registry(name: &str) -> SkillRegistry {
        let dir = std::env::temp_dir().join(format!(
            "avalon_skill_test_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        SkillRegistry::new(dir)
    }

    #[test]
    fn parse解析frontmatter与正文() {
        let text = "---\nname: github-pr\ndescription: 写 PR 时用\n---\n第一步\n第二步\n";
        let p = parse_skill(text);
        assert_eq!(p.name, "github-pr");
        assert_eq!(p.description, "写 PR 时用");
        assert_eq!(p.content, "第一步\n第二步");
    }

    #[test]
    fn parse无frontmatter时整篇当正文() {
        let text = "没有 frontmatter\n直接是正文\n";
        let p = parse_skill(text);
        assert_eq!(p.name, "");
        assert_eq!(p.description, "");
        assert_eq!(p.content, "没有 frontmatter\n直接是正文");
    }

    #[test]
    fn list扫描目录与get读正文() {
        let r = temp_registry("list_get");
        r.create("github-pr", "创建 PR 时用", "步骤一\n步骤二").unwrap();
        r.create("code-review", "审阅代码时用", "检查点\n").unwrap();

        let skills = r.list().unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "code-review"); // 按 name 排序

        let d = r.get("github-pr").unwrap();
        assert_eq!(d.description, "创建 PR 时用");
        assert_eq!(d.content, "步骤一\n步骤二");
        assert!(d.references.is_empty());
    }

    #[test]
    fn create已存在报错() {
        let r = temp_registry("dup");
        r.create("a", "d", "c").unwrap();
        assert!(r.create("a", "d", "c").is_err());
    }

    #[test]
    fn update改内容name只读() {
        let r = temp_registry("update");
        r.create("a", "旧描述", "旧内容").unwrap();
        let s = r.update("a", "新描述", "新内容").unwrap();
        assert_eq!(s.description, "新描述");
        assert_eq!(r.get("a").unwrap().content, "新内容");
        assert!(r.update("不存在", "d", "c").is_err());
    }

    #[test]
    fn delete删整目录() {
        let r = temp_registry("delete");
        r.create("a", "d", "c").unwrap();
        assert!(r.delete("a").is_ok());
        assert!(!r.list().unwrap().iter().any(|s| s.name == "a"));
        assert!(r.delete("a").is_err());
    }

    #[test]
    fn 非法名称被拒() {
        let r = temp_registry("badname");
        assert!(r.create("../evil", "d", "c").is_err());
        assert!(r.create("a/b", "d", "c").is_err());
        assert!(r.create("", "d", "c").is_err());
    }

    #[test]
    fn install写入references并可读取() {
        let r = temp_registry("install");
        let refs = vec![
            ("auth.md".to_string(), "认证说明\n".to_string()),
            ("sub/advanced.md".to_string(), "进阶\n".to_string()),
        ];
        let s = r
            .install("figma", "导入 Figma", "正文", &refs)
            .unwrap();
        assert_eq!(s.name, "figma");

        let refs_list = r.list_references("figma").unwrap();
        assert_eq!(refs_list, vec!["auth.md".to_string(), "sub/advanced.md".to_string()]);

        assert_eq!(r.read_reference("figma", "auth.md").unwrap(), "认证说明\n");
        assert_eq!(r.read_reference("figma", "sub/advanced.md").unwrap(), "进阶\n");
    }

    #[test]
    fn read_reference拒绝路径穿越() {
        let r = temp_registry("refpath");
        r.install("a", "d", "c", &[("x.md".to_string(), "x".to_string())])
            .unwrap();
        assert!(r.read_reference("a", "../evil.md").is_err());
        assert!(r.read_reference("a", "/abs.md").is_err());
    }
}
