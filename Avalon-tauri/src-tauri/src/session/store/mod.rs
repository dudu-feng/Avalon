// FileSessionStore：会话文件 CRUD + 压缩 + 渐进式总结 + 限界上下文 + rebuild 编排
//
// 存储布局：current/{channel}.json（活跃）+ history/{id}/index.json（归档）+ history/{id}/raw/{chunk}.json（原始消息）。
// 依赖注入：config（动态读路径/阈值）+ llm（压缩时动态构建 client）+ vector（摘要入库）。

#![allow(dead_code)] // session 模块供未来 engine/tool 引用，当前无调用方，接入后移除

mod compress_ops;
mod files;
mod history;
mod index;
mod migration;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::ConfigStore;
use crate::llm::LlmState;
use crate::vector::{RebuildProgress, RebuildStats, VectorStore};

use super::context::ContextBudget;
use super::types::*;
use super::{artifact, ArtifactRead};
use super::SessionStore;

use files::ensure_safe_session_id;

/// 旧会话工具链清洗标记：存在即表示已扫描过，无需每次启动重复读全部历史
const TOOLCHAIN_CLEANUP_MARKER: &str = ".toolchain_cleanup_v1";

/// 统一会话的唯一活跃槽位文件（current/main.json）。
/// channel 只作为消息属性存在，不再参与文件分区。
pub(crate) const ACTIVE_SESSION_KEY: &str = "main";

/// 统一会话迁移标记：存在即表示旧 current 已归档合并完毕
const UNIFIED_SESSION_MARKER: &str = ".unified_session_v1";

/// 会话文件存储实现
pub struct FileSessionStore {
    config: ConfigStore,
    llm: LlmState,
    vector: Arc<dyn VectorStore>,
}

impl FileSessionStore {
    pub fn new(config: ConfigStore, llm: LlmState, vector: Arc<dyn VectorStore>) -> Self {
        Self { config, llm, vector }
    }

    /// 从 session_id 提取时间戳（去掉任意 `{prefix}_` 前缀）
    ///
    /// 旧 id 可能以 app / feishu / qq 开头，迁移后仍保留原 id，不能按 main_ 硬切。
    fn doc_timestamp(&self, session_id: &str) -> String {
        session_id
            .split_once('_')
            .map(|(_, ts)| ts)
            .unwrap_or(session_id)
            .to_string()
    }

    /// 标题为「初始时间戳占位」或空时，用首条 user 消息截断生成（归档前调用，因 compress 会清空 messages）
    fn ensure_title(&self) -> Result<()> {
        let mut data = self.read_current()?;
        if data.id.is_empty() {
            return Ok(());
        }
        // 标题为空或等于初始时间戳占位（用户未手动改名）时，才用首条消息覆盖
        let default_ts = self.doc_timestamp(&data.id);
        if !data.title.is_empty() && data.title != default_ts {
            return Ok(());
        }
        let first = data.messages.iter().find_map(|m| match m {
            Message::User { content, .. } => Some(content.clone()),
            _ => None,
        });
        if let Some(content) = first {
            data.title = truncate_title(&content);
            self.write_current(&data)?;
        }
        Ok(())
    }

    /// 按 id 找到会话：优先 current（活跃副本比 history index 新），否则 history index
    fn locate_session(&self, id: &str) -> Result<SessionData> {
        let current = self.session_path().join("current");
        if current.is_dir() {
            for entry in std::fs::read_dir(&current).with_context(|| "读取 current 目录失败")? {
                let path = entry.context("读取 current 条目失败")?.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(data) = self.read_session_file(&path) {
                    if data.id == id {
                        return Ok(data);
                    }
                }
            }
        }

        let index = self.history_dir().join(id).join("index.json");
        if index.is_file() {
            return self.read_session_file(&index);
        }
        Err(anyhow::anyhow!("会话 '{id}' 不存在"))
    }

    /// 全部会话元信息：history 存档 + current 活跃，按 id 倒序去重
    fn all_session_metas(&self) -> Result<Vec<SessionMeta>> {
        let mut by_id: HashMap<String, SessionMeta> = HashMap::new();

        let history = self.history_dir();
        if history.is_dir() {
            for entry in std::fs::read_dir(&history).with_context(|| "读取 history 目录失败")? {
                let dir = entry.context("读取 history 条目失败")?.path();
                if !dir.is_dir() {
                    continue;
                }
                let index = dir.join("index.json");
                if !index.is_file() {
                    continue;
                }
                if let Ok(data) = self.read_session_file(&index) {
                    if !data.id.is_empty() {
                        by_id.insert(data.id.clone(), meta_from(&data));
                    }
                }
            }
        }

        let current = self.session_path().join("current");
        if current.is_dir() {
            for entry in std::fs::read_dir(&current).with_context(|| "读取 current 目录失败")? {
                let path = entry.context("读取 current 条目失败")?.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(data) = self.read_session_file(&path) {
                    if data.status == SessionStatus::Active && !data.id.is_empty() {
                        by_id.insert(data.id.clone(), meta_from(&data));
                    }
                }
            }
        }

        let mut out: Vec<SessionMeta> = by_id.into_values().collect();
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

}

#[async_trait]
impl SessionStore for FileSessionStore {
    fn init_session(&self) -> Result<()> {
        let data = self.read_current()?;
        if data.status == SessionStatus::Active && !data.id.is_empty() {
            log::debug!(target: "session", "已识别到上次会话，继续上次对话。");
            return Ok(());
        }
        self.create_active_session()?;
        Ok(())
    }

    async fn create_session(&self) -> Result<SessionData> {
        // ① 归档当前（若非空；空会话会被跳过）
        let current = self.read_current()?;
        if current.status == SessionStatus::Active && !current.id.is_empty() {
            self.save_current_session().await?;
        }
        // ② 新建 active 会话（写 current + history 初始存档）
        self.create_active_session()
    }

    fn get_current_session(&self) -> Result<SessionData> {
        self.read_current()
    }

    fn get_context_usage(&self) -> Result<ContextUsage> {
        let data = self.read_current()?;
        let cfg = self.config.get();
        let budget =
            ContextBudget::from_config(cfg.active_model_config(), &cfg.session_memory);
        // 优先展示下一次请求的锚点（真实 input + output，与压缩触发同一口径）；
        // 旧会话没有 stats 时回退到历史最大值，避免升级后进度条恒为 0。
        let used_tokens = if data.stats.last_input_tokens > 0 {
            data.stats
                .last_input_tokens
                .saturating_add(data.stats.last_output_tokens)
        } else {
            max_input_tokens(&data.messages)
        };
        Ok(ContextUsage {
            used_tokens,
            threshold: budget.trigger_tokens,
        })
    }

    fn update_current_session(&self, chat_history: &[Message]) -> Result<()> {
        let mut data = self.ensure_active()?;
        data.messages.extend_from_slice(chat_history);
        self.write_current(&data)
    }

    fn record_request_usage(
        &self,
        model: &str,
        usage: &crate::llm::TokenUsage,
    ) -> Result<()> {
        let mut data = self.read_current()?;
        if data.id.is_empty() {
            return Ok(());
        }

        data.stats = SessionStats {
            last_input_tokens: usage.input_tokens as usize,
            last_output_tokens: usage.output_tokens as usize,
            last_model: model.to_string(),
            last_request_at: chrono::Local::now().to_rfc3339(),
        };
        self.write_current(&data)
    }

    fn set_current_title(&self, title: &str) -> Result<()> {
        let mut data = self.ensure_active()?;
        if data.title == title {
            return Ok(());
        }
        data.title = title.to_string();
        self.write_current(&data)
    }

    async fn auto_compress_check(
        &self,
        anchor_tokens: usize,
        force: bool,
    ) -> Result<bool> {
        let cfg = self.config.get();
        let budget =
            ContextBudget::from_config(cfg.active_model_config(), &cfg.session_memory);
        if force || anchor_tokens >= budget.trigger_tokens {
            log::info!(
                target: "session",
                "输入锚点({anchor_tokens}) >= 触发线({})（force={force}），触发自动压缩...",
                budget.trigger_tokens
            );
            self.compress().await
        } else {
            Ok(false)
        }
    }

    async fn compress_session(&self) -> Result<bool> {
        // 空会话无需压缩（compress 内部也会短路）
        self.compress().await
    }

    async fn save_current_session(&self) -> Result<()> {
        let mut data = self.read_current()?;
        if data.id.is_empty() {
            log::debug!(target: "session", "当前无会话可归档。");
            return Ok(());
        }

        // 空会话（从未有内容）：不归档，清理 history 占位目录 + 重置 current
        if data.messages.is_empty() && data.compressed.is_empty() && data.super_compressed.is_none() {
            let dir = self.history_dir().join(&data.id);
            if dir.is_dir() {
                std::fs::remove_dir_all(&dir)
                    .with_context(|| format!("清理空会话目录失败: {}", dir.display()))?;
            }
            self.write_current(&SessionData::empty())?;
            log::debug!(target: "session", "当前会话为空，跳过归档。");
            return Ok(());
        }

        self.ensure_title()?;
        // 归档时把全部未压缩消息压成摘要；压缩失败时保留原文随 index 一起存档。
        // 压缩失败不能阻断归档：否则模型没配好时连新建 / 切换会话都做不了。
        if let Err(e) = self.compress().await {
            log::warn!(target: "session", "归档压缩失败，保留未压缩原文: {e:#}");
        }
        data = self.read_current()?;
        data.status = SessionStatus::Archived;
        self.write_session_index(&data)?;
        self.write_current(&SessionData::empty())
    }

    fn rebuild_index(
        &self,
        on_progress: &(dyn Fn(RebuildProgress) + Send + Sync),
    ) -> Result<RebuildStats> {
        FileSessionStore::rebuild_index(self, on_progress)
    }

    fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        let mut out = Vec::new();
        let history = self.history_dir();
        if history.is_dir() {
            for entry in
                std::fs::read_dir(&history).with_context(|| "读取 history 目录失败")?
            {
                let dir = entry.context("读取 history 条目失败")?.path();
                if !dir.is_dir() {
                    continue;
                }
                let index = dir.join("index.json");
                if index.is_file() {
                    if let Ok(data) = self.read_session_file(&index) {
                        // 统一会话：所有来源共用一个列表，不再按 channel 前缀过滤
                        out.push(meta_from(&data));
                    }
                }
            }
        }
        // 按 id 倒序（id 字典序 = 时间序，最新在前；前端再自行置顶 active）
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    async fn switch_session(&self, id: &str) -> Result<SessionData> {
        ensure_safe_session_id(id)?;
        // ① 目标会话必须存在（archived index）
        let src = self.history_dir().join(id).join("index.json");
        if !src.is_file() {
            return Err(anyhow::anyhow!("会话 '{id}' 不存在"));
        }

        // 已是当前会话则直接返回（防御性，前端已拦截）
        let current = self.read_current()?;
        if current.id == id {
            return Ok(current);
        }

        let mut target = self.read_session_file(&src)?;

        // ② 归档当前（若非空）
        if current.status == SessionStatus::Active && !current.id.is_empty() {
            self.save_current_session().await?;
        }

        // ③ 目标 status 改 active：写回 history index（存档备份，可容错回档）+ 写 current（进行中）
        target.status = SessionStatus::Active;
        self.write_session_index(&target)?;
        self.write_current(&target)?;
        Ok(target)
    }

    fn load_session_history(
        &self,
        id: &str,
        before_chunk: Option<u64>,
    ) -> Result<LoadHistoryResult> {
        ensure_safe_session_id(id)?;
        let (chunk, messages, has_earlier) = self.load_history_chunk(id, before_chunk)?;
        Ok(LoadHistoryResult { chunk, messages, has_earlier })
    }

    fn list_all_sessions(&self) -> Result<Vec<SessionMeta>> {
        self.all_session_metas()
    }

    fn read_session_raw_messages(&self, id: &str) -> Result<Vec<Message>> {
        ensure_safe_session_id(id)?;
        let data = self.locate_session(id)?;
        let mut messages = Vec::new();
        for n in self.raw_chunk_nums(id)? {
            let path = self.history_dir().join(id).join("raw").join(format!("{n}.json"));
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("读取 raw 文件失败: {}", path.display()))?;
            let chunk = serde_json::from_str::<RawChunkContent>(&content)
                .with_context(|| format!("解析 raw 文件失败: {}", path.display()))?
                .into_messages();
            messages.extend(chunk);
        }
        // raw 块是压缩前的历史，data.messages 是尚未压缩的尾部
        messages.extend(data.messages);
        Ok(messages)
    }

    fn write_tool_artifact(
        &self,
        session_id: &str,
        artifact_id: &str,
        content: &str,
    ) -> Result<ArtifactRef> {
        artifact::write(&self.session_path(), session_id, artifact_id, content)
    }

    fn read_tool_artifact(
        &self,
        artifact_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ArtifactRead> {
        let data = self.read_current()?;
        if data.id.is_empty() {
            anyhow::bail!("当前没有进行中的会话");
        }
        artifact::read(&self.session_path(), &data.id, artifact_id, offset, limit)
    }

    fn delete_session(&self, id: &str) -> Result<()> {
        ensure_safe_session_id(id)?;
        let dir = self.history_dir().join(id);
        let index = dir.join("index.json");

        // 仅归档会话可删除（active 会话的 index 在 current / 或为占位，raw 需保留）
        let archived = index.is_file()
            && self
                .read_session_file(&index)
                .map(|d| d.status == SessionStatus::Archived)
                .unwrap_or(false);
        if !archived {
            return Err(anyhow::anyhow!("会话 '{id}' 非归档状态，无法删除"));
        }

        // ① 清理向量库该会话 chunk
        if let Ok(data) = self.read_session_file(&index) {
            let mut ids: Vec<String> = data
                .compressed
                .iter()
                .map(|c| format!("{id}_chunk_{}", c.chunk))
                .collect();
            if let Some(sc) = &data.super_compressed {
                ids.push(format!("{id}_chunk_{}", sc.chunk));
            }
            let _ = self.vector.batch_delete(&ids);
        }

        // ② 删目录
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("删除会话目录失败: {}", dir.display()))?;
        }

        // ③ 删该会话的工具结果 artifact（与 history 分开存放，需单独清理）
        let artifacts = self.session_path().join("artifacts").join(id);
        if artifacts.is_dir() {
            std::fs::remove_dir_all(&artifacts)
                .with_context(|| format!("删除工具结果目录失败: {}", artifacts.display()))?;
        }
        Ok(())
    }

    fn rename_session(&self, id: &str, title: &str) -> Result<()> {
        ensure_safe_session_id(id)?;
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(anyhow::anyhow!("标题不能为空"));
        }

        // ① 改 history 存档（所有会话 index 都在 history，是唯一权威）
        let path = self.history_dir().join(id).join("index.json");
        if !path.is_file() {
            return Err(anyhow::anyhow!("会话 '{id}' 不存在"));
        }
        let mut data = self.read_session_file(&path)?;
        data.title = title.clone();
        self.write_session_index(&data)?;

        // ② active 会话：同步 current（进行中副本）
        let current = self.read_current()?;
        if current.id == id {
            let mut c = current;
            c.title = title;
            self.write_current(&c)?;
        }
        Ok(())
    }
}

/// 遍历消息取最大 input_tokens（只有 assistant 消息带 token_usage）
pub(crate) fn max_input_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant { token_usage, .. } => Some(token_usage.input_tokens as usize),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// 从 chunk 字符串提取块编号："" 或 "merged_1_5" → 数字列表
pub(crate) fn extract_chunk_nums(chunk: &str) -> Vec<u64> {
    if let Some(rest) = chunk.strip_prefix("merged_") {
        rest.split('_').filter_map(|p| p.parse().ok()).collect()
    } else {
        chunk.parse().ok().into_iter().collect()
    }
}

/// raw/{chunk}.json 的信封结构（旧数据是裸 `Vec<Message>` 数组，读取时兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawChunkFile {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    chunk: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    artifact_ids: Vec<String>,
}

/// raw 文件内容：信封（新）或裸消息数组（旧）
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawChunkContent {
    Envelope(RawChunkFile),
    Legacy(Vec<Message>),
}

impl RawChunkContent {
    pub(crate) fn into_messages(self) -> Vec<Message> {
        match self {
            RawChunkContent::Envelope(f) => f.messages,
            RawChunkContent::Legacy(messages) => messages,
        }
    }
}

/// 按插入顺序去重
fn dedup_keep_order(items: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

/// 标题截断：首条消息前 N 字，超长加省略号（按字符，兼容中文）
fn truncate_title(s: &str) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(20).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out.trim().to_string()
}

/// 从 session_id 解析创建时间（epoch 秒）。id 形如 {channel}_{%Y-%m-%d-%H_%M_%S}
fn id_epoch(id: &str) -> i64 {
    let ts = id.split_once('_').map(|x| x.1).unwrap_or(id);
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d-%H_%M_%S")
        .ok()
        .and_then(|naive| naive.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

/// 从会话数据构建列表元信息（标题空则回退时间戳）
fn meta_from(data: &SessionData) -> SessionMeta {
    let title = if data.title.is_empty() {
        data.id.split_once('_').map(|x| x.1)
            .unwrap_or(&data.id)
            .to_string()
    } else {
        data.title.clone()
    };
    SessionMeta {
        id: data.id.clone(),
        title,
        status: data.status,
        created_at: id_epoch(&data.id),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::files::temp_path;
    use super::*;

    /// 会话 ID 会直接拼进 history/{id}/...：必须挡住路径穿越与绝对路径。
    #[test]
    fn ensure_safe_session_id_rejects_path_traversal() {
        for id in [
            "",
            ".",
            "..",
            "../evil",
            "..\\evil",
            "a/b",
            "a\\b",
            "C:\\evil",
            "a b",
            "会话_1",
        ] {
            assert!(
                ensure_safe_session_id(id).is_err(),
                "{id:?} 应被拒绝"
            );
        }

        for id in [
            "main_2026-11-28-10_00_00",
            "terminal_2026-08-17-10_00_00",
            "abc-123",
        ] {
            assert!(
                ensure_safe_session_id(id).is_ok(),
                "{id:?} 应被接受"
            );
        }
    }

    /// 同目标并发写的临时文件必须互不相同，否则会互相截断。
    #[test]
    fn temp_path_is_unique_per_call() {
        let base = Path::new("session.json");
        let a = temp_path(base);
        let b = temp_path(base);
        assert_ne!(a, b);
        assert!(a.to_string_lossy().ends_with(".tmp"));
    }
}
