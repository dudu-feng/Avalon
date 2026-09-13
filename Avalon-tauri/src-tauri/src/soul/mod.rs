// 灵魂注册表模块
//
// 统一管理智能体的「灵魂」（自我设定）与「用户画像」（关于主人的长期记忆），
// 二者作为条目（entry）管理，是 system prompt 的持久化来源。
//
// 存储解耦（索引 / 正文分离）：
//   - memory/prompt/registry.json 只存条目索引（id / kind / category / title / order / enabled
//     + content_file），不含正文 —— 保持轻量、可读，直接支撑前端一级列表渲染。
//   - 每条目正文存独立 md：memory/prompt/entries/{id}.md，由 content_file 字段指向。
//     改正文只动 md 文件、不碰索引；一级列表（元数据）与二级详情（正文）按需读取。
//
// 设计要点（条目化取代分节 markdown）：
//   - kind 分层：system 系统注入（完全只读，且不在前后端展示，仅作默认注入）、
//     seed 初始化推荐（可编辑不可删）、user 用户或 agent 新增（可增删改）。
//   - enabled 控制是否注入 system prompt（前端 switch、agent 工具 set_soul_entry_enabled）。
//   - 自进化：agent 通过 list/get/create/update/delete/set_enabled 工具维护条目；前端可查看编辑。
//   - PromptAssembler 读本注册表拼 system prompt（含 system 基本设定），不扫目录 .md、不硬编码。
//   - 首次启动从旧分节 .md（soul.md / user_profile.md）迁移 seed 条目并删除旧文件；
//     旧版 registry.json（内嵌 content）也会自动迁移为「索引 + 独立 md」。

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

/// 系统注入条目（基本设定）的标题
pub const SYSTEM_BASIC_TITLE: &str = "基本设定";
/// 系统注入条目（基本设定）的内容（原 templates.rs BASIC_SETTING 去掉首行标题）
pub const SYSTEM_BASIC_CONTENT: &str = "你是智能体Avalon，是由 dudu-feng 开发的一款智能体，开发者对 Avalon 有以下期望：\n- 为什么取 Avalon 这个名字：Avalon 是传说中遗世独立的理想乡，意在用户能在使用 Avalon 的过程中创造属于自己的智能体理想乡。\n- \"make your own Avalon\"——\"创造属于你自己的 Avalon\"";

/// 灵魂默认节（旧 soul.md 迁移种子，文件缺失/为空时兜底）
const SOUL_SEED: &[(&str, &str)] = &[
    ("身份", "<!-- 你是谁：名字、角色、定位 -->"),
    ("设定", "<!-- 性格、语气、价值观、行事原则、能力边界 -->"),
    ("约束", "<!-- 硬性约束、红线，不可违背 -->"),
];

/// 画像默认节（旧 user_profile.md 迁移种子）
const PROFILE_SEED: &[(&str, &str)] = &[
    ("基本信息", "<!-- 称呼、身份、职业、所在地等 -->"),
    ("偏好与习惯", "<!-- 喜好、沟通偏好、作息等 -->"),
    ("目标与关注", "<!-- 当前在做什么、关注什么、短期目标 -->"),
    ("关系与协作", "<!-- 与智能体的协作方式、期望 -->"),
    ("重要背景", "<!-- 过往重要事件、上下文 -->"),
];

/// 旧分节文件（迁移源，迁移后删除）
const LEGACY_SOUL_FILE: &str = "soul.md";
const LEGACY_PROFILE_FILE: &str = "user_profile.md";

/// 正文文件存放子目录（相对 prompt_dir）
const ENTRIES_DIR: &str = "entries";

/// 条目保护级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    /// 系统注入，完全只读（agent 工具与前端都不可改、不展示）
    System,
    /// 初始化推荐条目，可编辑不可删除（保留骨架）
    Seed,
    /// 用户 / agent 新增，可增删改
    User,
}

/// 条目类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryCategory {
    /// 灵魂（智能体的自我）
    Soul,
    /// 用户画像（关于主人的长期记忆）
    Profile,
}

impl EntryCategory {
    /// 工具/命令参数 category → 类别（仅接受 soul / profile，非法返回 None）
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "soul" => Some(Self::Soul),
            "profile" => Some(Self::Profile),
            _ => None,
        }
    }
}

/// 一条灵魂/画像条目（索引，不含正文）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulEntry {
    pub id: String,
    pub kind: EntryKind,
    pub category: EntryCategory,
    pub title: String,
    /// 正文文件路径（相对 prompt_dir），正文不再内嵌在 registry.json
    pub content_file: String,
    pub order: u32,
    pub enabled: bool,
}

/// 单条条目 + 正文（get 接口返回，供前端模态框 / agent 查看内容）
#[derive(Debug, Clone, Serialize)]
pub struct SoulEntryDetail {
    #[serde(flatten)]
    pub entry: SoulEntry,
    pub content: String,
}

/// 灵魂注册表：registry.json 索引 + entries/*.md 正文的读写、条目增删改查、组装注入片段。
/// 索引文件小，不缓存；写操作串行化（Mutex），读不锁（原子 rename 保证读到完整版本）。
pub struct SoulRegistry {
    prompt_dir: PathBuf,
    path: PathBuf,
    /// 变更历史文件（None = 不记录，测试与轻量场景用）
    history_path: Option<PathBuf>,
    write_lock: Mutex<()>,
}

impl SoulRegistry {
    /// 从 prompt 目录构造（registry.json 与旧 .md 同目录，即 data_root/memory/prompt）
    pub fn new(prompt_dir: PathBuf) -> Self {
        let path = prompt_dir.join("registry.json");
        Self {
            prompt_dir,
            path,
            history_path: None,
            write_lock: Mutex::new(()),
        }
    }

    /// 开启变更历史（条目增删改时追加一行到指定文件，如 data_root/logs/soul_history.md）。
    /// 历史文件不能放 prompt 目录，否则会被拼进 system prompt。
    pub fn with_history(mut self, path: PathBuf) -> Self {
        self.history_path = Some(path);
        self
    }

    /// 初始化 + 迁移（幂等）：
    ///   1. registry.json 缺失 → 从旧分节 .md 迁移 seed 条目 + 写入 system 条目，删旧 .md；
    ///   2. registry.json 存在但内嵌 content（旧版）→ 把正文拆到 entries/*.md，重写为索引；
    ///   3. 已是「索引 + 独立 md」→ 跳过。
    pub fn init(&self) -> Result<()> {
        if !self.path.exists() {
            let items = self.build_initial_entries()?;
            let mut entries = Vec::with_capacity(items.len());
            for (entry, content) in items {
                self.write_content(&entry.content_file, &content)?;
                entries.push(entry);
            }
            self.write_all(&entries)?;
            for legacy in [LEGACY_SOUL_FILE, LEGACY_PROFILE_FILE] {
                let p = self.prompt_dir.join(legacy);
                if p.exists() {
                    let _ = std::fs::remove_file(&p);
                }
            }
            return Ok(());
        }
        if self.needs_content_migration()? {
            self.migrate_content_to_files()?;
        }
        Ok(())
    }

    /// 读取全部条目（按 order 排序，含 system）；文件不存在返回空列表
    pub fn list(&self) -> Result<Vec<SoulEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path)
            .with_context(|| format!("读取灵魂注册表失败: {}", self.path.display()))?;
        let mut entries: Vec<SoulEntry> = serde_json::from_str(&raw)
            .with_context(|| format!("解析灵魂注册表失败: {}", self.path.display()))?;
        entries.sort_by_key(|e| e.order);
        Ok(entries)
    }

    /// 读取「可见」条目（不含 system）—— 前端与 agent 工具的展示/编辑对象。
    /// system 基本设定只作为默认注入，不出现在任何编辑界面。
    pub fn list_visible(&self) -> Result<Vec<SoulEntry>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|e| e.kind != EntryKind::System)
            .collect())
    }

    /// 读取单条条目 + 正文
    pub fn get(&self, id: &str) -> Result<SoulEntryDetail> {
        let entries = self.list()?;
        let entry = entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow!("条目 '{id}' 不存在"))?;
        let content = self.read_content(&entry.content_file)?;
        Ok(SoulEntryDetail { entry, content })
    }

    /// 新增条目（kind=user），order 排到末尾；先写正文 md 再登记索引
    pub fn create(
        &self,
        category: EntryCategory,
        title: &str,
        content: &str,
    ) -> Result<SoulEntry> {
        let title = title.trim();
        let content = content.trim();
        if title.is_empty() {
            return Err(anyhow!("缺少 title（条目标题）"));
        }
        if content.is_empty() {
            return Err(anyhow!("缺少 content（条目内容）"));
        }

        let _guard = self.write_lock.lock().unwrap();
        let mut entries = self.list()?;
        let next_order = entries.iter().map(|e| e.order).max().map_or(0, |o| o + 1);
        let id = self.new_id(category);
        let entry = SoulEntry {
            id: id.clone(),
            kind: EntryKind::User,
            category,
            title: title.to_string(),
            content_file: Self::content_file_for(&id),
            order: next_order,
            enabled: true,
        };
        self.write_content(&entry.content_file, content)?;
        entries.push(entry.clone());
        self.write_all(&entries)?;
        self.record(&format!("新增「{}」", entry.title), content, None);
        Ok(entry)
    }

    /// 编辑条目（title/content 至少传一个），system 只读；改正文只动 md 文件
    pub fn update(
        &self,
        id: &str,
        title: Option<&str>,
        content: Option<&str>,
    ) -> Result<SoulEntry> {
        let _guard = self.write_lock.lock().unwrap();
        let mut entries = self.list()?;
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow!("条目 '{id}' 不存在"))?;
        if entry.kind == EntryKind::System {
            return Err(anyhow!("系统条目只读，不可修改"));
        }

        let mut old_content: Option<String> = None;
        let mut new_content: Option<String> = None;
        if let Some(t) = title {
            let t = t.trim();
            if t.is_empty() {
                return Err(anyhow!("title 不能为空"));
            }
            entry.title = t.to_string();
        }
        if let Some(c) = content {
            let c = c.trim();
            if c.is_empty() {
                return Err(anyhow!("content 不能为空"));
            }
            old_content = Some(self.read_content(&entry.content_file)?);
            self.write_content(&entry.content_file, c)?;
            new_content = Some(c.to_string());
        }

        let updated = entry.clone();
        self.write_all(&entries)?;
        let new_for_history = new_content
            .or(old_content)
            .unwrap_or_else(|| self.read_content(&updated.content_file).unwrap_or_default());
        self.record(
            &format!("更新「{}」", updated.title),
            &new_for_history,
            None,
        );
        Ok(updated)
    }

    /// 删除条目，system 与 seed 不可删；先删索引再删正文（孤儿 md 无害，best-effort）
    pub fn delete(&self, id: &str) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap();
        let mut entries = self.list()?;
        let idx = entries
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| anyhow!("条目 '{id}' 不存在"))?;
        match entries[idx].kind {
            EntryKind::System => return Err(anyhow!("系统条目只读，不可删除")),
            EntryKind::Seed => return Err(anyhow!("种子条目保留骨架，不可删除")),
            EntryKind::User => {}
        }
        let removed = entries.remove(idx);
        self.write_all(&entries)?;
        let content = self.read_content(&removed.content_file).unwrap_or_default();
        let _ = std::fs::remove_file(self.content_path(&removed.content_file));
        self.record(&format!("删除「{}」", removed.title), &content, None);
        Ok(())
    }

    /// 切换条目是否注入 system prompt（enabled），system 只读
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<SoulEntry> {
        let _guard = self.write_lock.lock().unwrap();
        let mut entries = self.list()?;
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow!("条目 '{id}' 不存在"))?;
        if entry.kind == EntryKind::System {
            return Err(anyhow!("系统条目只读，不可修改注入状态"));
        }
        entry.enabled = enabled;
        let updated = entry.clone();
        self.write_all(&entries)?;
        self.record(
            &format!("{}「{}」的注入", if enabled { "启用" } else { "停用" }, updated.title),
            "",
            None,
        );
        Ok(updated)
    }

    /// 组装注入片段：按 order 排序、跳过 disabled，统一渲染为 `## {title}\n{content}`
    pub fn assemble(&self) -> Result<Vec<String>> {
        let entries = self.list()?;
        let mut parts = Vec::new();
        for e in entries.into_iter().filter(|e| e.enabled) {
            let content = self.read_content(&e.content_file)?;
            parts.push(format!("## {}\n{}", e.title, content));
        }
        Ok(parts)
    }

    /// 从旧分节 .md 构建初始条目（system + seed），返回 (条目, 正文) 列表
    fn build_initial_entries(&self) -> Result<Vec<(SoulEntry, String)>> {
        let mut items = Vec::new();
        let mut order = 0u32;

        let basic_id = "system-basic".to_string();
        items.push((
            SoulEntry {
                id: basic_id.clone(),
                kind: EntryKind::System,
                category: EntryCategory::Soul,
                title: SYSTEM_BASIC_TITLE.to_string(),
                content_file: Self::content_file_for(&basic_id),
                order,
                enabled: true,
            },
            SYSTEM_BASIC_CONTENT.to_string(),
        ));
        order += 1;

        for (title, content) in self.legacy_sections(LEGACY_SOUL_FILE, SOUL_SEED) {
            let id = format!("seed-soul-{order}");
            items.push((
                SoulEntry {
                    id: id.clone(),
                    kind: EntryKind::Seed,
                    category: EntryCategory::Soul,
                    title,
                    content_file: Self::content_file_for(&id),
                    order,
                    enabled: true,
                },
                content,
            ));
            order += 1;
        }

        for (title, content) in self.legacy_sections(LEGACY_PROFILE_FILE, PROFILE_SEED) {
            let id = format!("seed-profile-{order}");
            items.push((
                SoulEntry {
                    id: id.clone(),
                    kind: EntryKind::Seed,
                    category: EntryCategory::Profile,
                    title,
                    content_file: Self::content_file_for(&id),
                    order,
                    enabled: true,
                },
                content,
            ));
            order += 1;
        }

        Ok(items)
    }

    /// 读取旧 .md 提取节；文件缺失/为空则返回内置种子
    fn legacy_sections(&self, file: &str, fallback: &[(&str, &str)]) -> Vec<(String, String)> {
        let path = self.prompt_dir.join(file);
        let fallback = fallback
            .iter()
            .map(|(t, c)| (t.to_string(), c.to_string()))
            .collect::<Vec<_>>();
        match std::fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => {
                let sections = extract_sections(&text);
                if sections.is_empty() {
                    fallback
                } else {
                    sections
                }
            }
            _ => fallback,
        }
    }

    fn new_id(&self, category: EntryCategory) -> String {
        let prefix = match category {
            EntryCategory::Soul => "soul",
            EntryCategory::Profile => "profile",
        };
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{prefix}-{nanos}")
    }

    /// 正文文件相对路径（相对 prompt_dir）
    fn content_file_for(id: &str) -> String {
        format!("{ENTRIES_DIR}/{id}.md")
    }

    fn content_path(&self, content_file: &str) -> PathBuf {
        self.prompt_dir.join(content_file)
    }

    fn read_content(&self, content_file: &str) -> Result<String> {
        let p = self.content_path(content_file);
        std::fs::read_to_string(&p)
            .with_context(|| format!("读取灵魂条目正文失败: {}", p.display()))
    }

    fn write_content(&self, content_file: &str, content: &str) -> Result<()> {
        let p = self.content_path(content_file);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建灵魂条目正文目录失败: {}", parent.display()))?;
        }
        std::fs::write(&p, content)
            .with_context(|| format!("写入灵魂条目正文失败: {}", p.display()))
    }

    /// 检测 registry.json 是否为旧版（内嵌 content 字段）。新格式只有 content_file，
    /// 精确匹配 `"content"`（带引号）即可与 `"content_file"` 区分。
    fn needs_content_migration(&self) -> Result<bool> {
        let raw = std::fs::read_to_string(&self.path)
            .with_context(|| format!("读取灵魂注册表失败: {}", self.path.display()))?;
        Ok(raw.contains("\"content\""))
    }

    /// 旧版 registry.json（内嵌 content）→ 索引 + 独立 md
    fn migrate_content_to_files(&self) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap();
        let raw = std::fs::read_to_string(&self.path)
            .with_context(|| format!("读取灵魂注册表失败: {}", self.path.display()))?;
        let legacy: Vec<LegacyEntry> = serde_json::from_str(&raw)
            .with_context(|| format!("解析旧版灵魂注册表失败: {}", self.path.display()))?;
        let mut entries = Vec::with_capacity(legacy.len());
        for e in legacy {
            let content_file = Self::content_file_for(&e.id);
            self.write_content(&content_file, &e.content)?;
            entries.push(SoulEntry {
                id: e.id,
                kind: e.kind,
                category: e.category,
                title: e.title,
                content_file,
                order: e.order,
                enabled: e.enabled,
            });
        }
        self.write_all(&entries)?;
        Ok(())
    }

    fn write_all(&self, entries: &[SoulEntry]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建灵魂注册表目录失败: {}", parent.display()))?;
        }
        let json =
            serde_json::to_string_pretty(entries).context("序列化灵魂注册表失败")?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .with_context(|| format!("写入灵魂注册表临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("原子替换灵魂注册表失败: {}", self.path.display()))?;
        Ok(())
    }

    /// 追加一条变更历史（best-effort：失败不影响主流程，历史只是审计辅助）
    fn record(&self, action: &str, new: &str, old: Option<&str>) {
        let Some(path) = &self.history_path else {
            return;
        };
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let mut entry = format!("\n--- {ts} · {action} ---\n");
        if let Some(old) = old {
            if !old.trim().is_empty() {
                entry.push_str(&format!("旧：{}\n", old.trim()));
            }
        }
        if !new.trim().is_empty() {
            entry.push_str(&format!("新：{}\n", new.trim()));
        }

        use std::io::Write;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }
}

/// 旧版条目（内嵌 content 字段），仅用于迁移反序列化
#[derive(Debug, Deserialize)]
struct LegacyEntry {
    id: String,
    kind: EntryKind,
    category: EntryCategory,
    title: String,
    #[serde(default)]
    content: String,
    order: u32,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 从 markdown 提取 `## title` 节（前置 `### ` 等归入丢弃，节标题行不进正文），
/// 返回 (标题, 正文) 列表。仅用于旧 .md 迁移，不做更新/重组。
pub fn extract_sections(text: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();

    for line in text.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(t) = current_title.take() {
                sections.push((t, current_body.trim_end().to_string()));
                current_body = String::new();
            }
            current_title = Some(title.trim().to_string());
        } else if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(t) = current_title.take() {
        sections.push((t, current_body.trim_end().to_string()));
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_registry(name: &str) -> SoulRegistry {
        let dir = std::env::temp_dir().join(format!(
            "avalon_soul_test_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        SoulRegistry::new(dir)
    }

    #[test]
    fn extract提取节() {
        let text = "### 头\n\n## 身份\n我是谁\n\n## 约束\n红线\n";
        let s = extract_sections(text);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], ("身份".to_string(), "我是谁".to_string()));
        assert_eq!(s[1], ("约束".to_string(), "红线".to_string()));
    }

    #[test]
    fn init生成system与seed() {
        let r = temp_registry("init");
        r.init().unwrap();
        let entries = r.list().unwrap();
        assert_eq!(entries[0].kind, EntryKind::System);
        assert_eq!(entries[0].title, SYSTEM_BASIC_TITLE);
        assert!(entries
            .iter()
            .any(|e| e.kind == EntryKind::Seed && e.category == EntryCategory::Soul));
        assert!(entries
            .iter()
            .any(|e| e.kind == EntryKind::Seed && e.category == EntryCategory::Profile));
    }

    #[test]
    fn init迁移旧md并删除旧文件() {
        let dir = std::env::temp_dir().join(format!(
            "avalon_soul_test_{}_migrate",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 预写旧分节 soul.md（模拟上一版数据结构）
        std::fs::write(dir.join("soul.md"), "### 头\n\n## 身份\n我是Avalon\n").unwrap();

        let r = SoulRegistry::new(dir.clone());
        r.init().unwrap();

        let entries = r.list().unwrap();
        let identity = entries.iter().find(|e| e.title == "身份").unwrap();
        assert_eq!(identity.kind, EntryKind::Seed);
        // 正文已外置到独立 md，通过 get 读取
        assert_eq!(r.get(&identity.id).unwrap().content, "我是Avalon");
        // 旧文件已迁移并删除，registry.json 成为索引真相源
        assert!(!dir.join("soul.md").exists());
        assert!(dir.join("registry.json").exists());
    }

    #[test]
    fn 迁移内嵌content到独立md() {
        let dir = std::env::temp_dir().join(format!(
            "avalon_soul_test_{}_content_migrate",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 预写旧版 registry.json（内嵌 content 字段）
        std::fs::write(
            dir.join("registry.json"),
            r#"[
                {"id":"system-basic","kind":"system","category":"soul","title":"基本设定","content":"设定正文","order":0,"enabled":true},
                {"id":"seed-soul-1","kind":"seed","category":"soul","title":"身份","content":"我是Avalon","order":1,"enabled":true}
            ]"#,
        )
        .unwrap();

        let r = SoulRegistry::new(dir.clone());
        r.init().unwrap();

        let entries = r.list().unwrap();
        assert_eq!(entries.len(), 2);
        // registry.json 不再含内嵌 content 键，改为 content_file
        let raw = std::fs::read_to_string(dir.join("registry.json")).unwrap();
        assert!(!raw.contains("\"content\""));
        assert!(raw.contains("content_file"));
        // 正文写入独立 md 并可读回
        let identity = entries.iter().find(|e| e.title == "身份").unwrap();
        assert_eq!(r.get(&identity.id).unwrap().content, "我是Avalon");
        assert!(dir.join("entries/seed-soul-1.md").exists());
    }

    #[test]
    fn create追加user条目到末尾() {
        let r = temp_registry("create");
        r.init().unwrap();
        let before = r.list().unwrap().len();
        let e = r.create(EntryCategory::Profile, "新条目", "内容").unwrap();
        assert_eq!(e.kind, EntryKind::User);
        let after = r.list().unwrap();
        assert_eq!(after.len(), before + 1);
        assert_eq!(after.last().unwrap().id, e.id);
        assert_eq!(r.get(&e.id).unwrap().content, "内容");
    }

    #[test]
    fn system只读不可改不可删() {
        let r = temp_registry("system");
        r.init().unwrap();
        let system_id = r.list().unwrap()[0].id.clone();
        assert!(r.update(&system_id, Some("x"), None).is_err());
        assert!(r.delete(&system_id).is_err());
    }

    #[test]
    fn seed可编辑不可删除() {
        let r = temp_registry("seed");
        r.init().unwrap();
        let seed_id = r
            .list()
            .unwrap()
            .iter()
            .find(|e| e.kind == EntryKind::Seed)
            .unwrap()
            .id
            .clone();
        let updated = r.update(&seed_id, Some("新标题"), Some("新内容")).unwrap();
        assert_eq!(updated.title, "新标题");
        assert_eq!(r.get(&seed_id).unwrap().content, "新内容");
        assert!(r.delete(&seed_id).is_err());
    }

    #[test]
    fn user可删除() {
        let r = temp_registry("user");
        r.init().unwrap();
        let e = r.create(EntryCategory::Soul, "临时", "内容").unwrap();
        assert!(r.delete(&e.id).is_ok());
        // 删除后正文文件一并清理
        let content_file = e.content_file.clone();
        assert!(!r.content_path(&content_file).exists());
    }

    #[test]
    fn assemble渲染为标题加正文() {
        let r = temp_registry("assemble");
        r.init().unwrap();
        let parts = r.assemble().unwrap();
        assert!(parts[0].starts_with("## 基本设定"));
        assert!(parts.iter().any(|p| p.contains("## 身份")));
    }

    #[test]
    fn list_visible过滤system() {
        let r = temp_registry("visible");
        r.init().unwrap();
        let all = r.list().unwrap();
        let visible = r.list_visible().unwrap();
        assert!(visible.iter().all(|e| e.kind != EntryKind::System));
        assert_eq!(all.len() - 1, visible.len());
    }

    #[test]
    fn set_enabled切换注入状态() {
        let r = temp_registry("toggle");
        r.init().unwrap();
        let seed = r.list_visible().unwrap()[0].clone();
        let title = seed.title.clone();

        let e = r.set_enabled(&seed.id, false).unwrap();
        assert!(!e.enabled);
        let parts = r.assemble().unwrap();
        assert!(!parts.iter().any(|p| p.starts_with(&format!("## {title}"))));

        let e = r.set_enabled(&seed.id, true).unwrap();
        assert!(e.enabled);
        let parts = r.assemble().unwrap();
        assert!(parts.iter().any(|p| p.starts_with(&format!("## {title}"))));

        let system_id = r
            .list()
            .unwrap()
            .iter()
            .find(|e| e.kind == EntryKind::System)
            .unwrap()
            .id
            .clone();
        assert!(r.set_enabled(&system_id, false).is_err());
    }
}
