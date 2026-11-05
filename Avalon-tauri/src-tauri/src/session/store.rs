// FileSessionStore：会话文件 CRUD + 压缩 + 渐进式总结 + 限界上下文 + rebuild 编排
//
// 存储布局：current/{channel}.json（活跃）+ history/{id}/index.json（归档）+ history/{id}/raw/{chunk}.json（原始消息）。
// 依赖注入：config（动态读路径/阈值）+ llm（压缩时动态构建 client）+ vector（摘要入库）。

#![allow(dead_code)] // session 模块供未来 engine/tool 引用，当前无调用方，接入后移除

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::ConfigStore;
use crate::llm::{CompressResult, LlmState};
use crate::prompt::{build_compress_prompt, build_rolling_prompt};
use crate::vector::{RebuildProgress, RebuildStats, VectorStore};

use super::compress::{build_transcript, message_time_range};
use super::context::{estimate_text_tokens, ContextBudget};
use super::types::*;
use super::{artifact, ArtifactRead};
use super::{now_id_ts, SessionStore};

/// 旧会话工具链清洗标记：存在即表示已扫描过，无需每次启动重复读全部历史
const TOOLCHAIN_CLEANUP_MARKER: &str = ".toolchain_cleanup_v1";

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

    // ============ 路径辅助 ============

    fn session_path(&self) -> PathBuf {
        self.config.get().session_path()
    }

    fn current_file(&self, channel: &str) -> PathBuf {
        self.session_path().join("current").join(format!("{channel}.json"))
    }

    fn history_dir(&self) -> PathBuf {
        self.session_path().join("history")
    }

    // ============ 文件读写 ============

    /// 读当前会话；文件不存在返回空会话（inactive）
    fn read_current(&self, channel: &str) -> Result<SessionData> {
        let path = self.current_file(channel);
        if !path.exists() {
            return Ok(SessionData::empty());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("读取会话文件失败: {}", path.display()))?;
        // 旧格式（扁平 ChatMessage / 三段标记）不再兼容，解析失败降级空会话，避免阻断启动
        match serde_json::from_str(&content) {
            Ok(data) => Ok(data),
            Err(e) => {
                log::warn!(target: "session", "旧格式会话已忽略（{}）: {e}", path.display());
                Ok(SessionData::empty())
            }
        }
    }

    /// 写当前会话（原子：tmp + rename，自动建目录）
    fn write_current(&self, channel: &str, data: &SessionData) -> Result<()> {
        let path = self.current_file(channel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建会话目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(data).context("序列化会话数据失败")?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("写入会话临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("原子替换会话文件失败: {}", path.display()))?;
        Ok(())
    }

    /// 确保会话就绪：active 则复用，否则新建（含 history 初始存档）
    fn ensure_active(&self, channel: &str) -> Result<SessionData> {
        let data = self.read_current(channel)?;
        if data.status == SessionStatus::Active && !data.id.is_empty() {
            return Ok(data);
        }
        self.create_active_session(channel)
    }

    /// 新建一个 active 会话：写 current + 写 history/{id}/index.json（初始存档）
    fn create_active_session(&self, channel: &str) -> Result<SessionData> {
        let fresh = SessionData::new_active(format!("{channel}_{}", now_id_ts()));
        self.write_current(channel, &fresh)?;
        self.write_session_index(&fresh)?;
        Ok(fresh)
    }

    /// 写会话 index 文件（history/{id}/index.json），自动建目录
    fn write_session_index(&self, data: &SessionData) -> Result<()> {
        let dir = self.history_dir().join(&data.id);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("创建会话目录失败: {}", dir.display()))?;
        let content = serde_json::to_string_pretty(data).context("序列化会话数据失败")?;
        std::fs::write(dir.join("index.json"), content).with_context(|| "写入 index.json 失败")
    }

    /// 一次性清洗旧版本取消留下的不完整工具链。
    ///
    /// 旧引擎停止时仍会写入已生成的部分轨迹，可能留下 assistant(tool_calls)
    /// 缺少对应 tool 结果的半条链。新投影不做运行时配对修复（见
    /// `context::build_history_messages`），因此启动时把 current / history index /
    /// raw 块统一清洗一次。标记文件保证只扫描一次；清洗幂等，失败时下次启动重试。
    pub fn cleanup_legacy_tool_chains(&self) -> Result<usize> {
        let marker = self.session_path().join(TOOLCHAIN_CLEANUP_MARKER);
        if marker.is_file() {
            return Ok(0);
        }

        let mut removed = 0usize;
        let mut touched = 0usize;

        // ① 活跃会话：current/{channel}.json
        let current_dir = self.session_path().join("current");
        if current_dir.is_dir() {
            for entry in std::fs::read_dir(&current_dir)
                .with_context(|| format!("读取 current 目录失败: {}", current_dir.display()))?
            {
                let path = entry.context("读取 current 条目失败")?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let Some(channel) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                match self.read_session_file(&path) {
                    Ok(mut data) => {
                        let n = super::migrate::sanitize_tool_chains(&mut data.messages);
                        if n > 0 {
                            self.write_current(channel, &data)?;
                            removed += n;
                            touched += 1;
                        }
                    }
                    Err(e) => log::warn!(
                        target: "session",
                        "跳过无法解析的会话文件 {}: {e:#}",
                        path.display()
                    ),
                }
            }
        }

        // ② 归档会话：history/{id}/index.json + raw/{chunk}.json
        let history = self.history_dir();
        if history.is_dir() {
            for entry in std::fs::read_dir(&history)
                .with_context(|| format!("读取 history 目录失败: {}", history.display()))?
            {
                let dir = entry.context("读取 history 条目失败")?.path();
                if !dir.is_dir() {
                    continue;
                }

                let index = dir.join("index.json");
                if index.is_file() {
                    match self.read_session_file(&index) {
                        Ok(mut data) => {
                            let n = super::migrate::sanitize_tool_chains(&mut data.messages);
                            if n > 0 {
                                self.write_session_index(&data)?;
                                removed += n;
                                touched += 1;
                            }
                        }
                        Err(e) => log::warn!(
                            target: "session",
                            "跳过无法解析的会话索引 {}: {e:#}",
                            index.display()
                        ),
                    }
                }

                let raw_dir = dir.join("raw");
                if raw_dir.is_dir() {
                    for raw_entry in std::fs::read_dir(&raw_dir)
                        .with_context(|| format!("读取 raw 目录失败: {}", raw_dir.display()))?
                    {
                        let path = raw_entry.context("读取 raw 条目失败")?.path();
                        if path.extension().and_then(|s| s.to_str()) != Some("json") {
                            continue;
                        }
                        match self.cleanup_raw_file(&path) {
                            Ok(n) if n > 0 => {
                                removed += n;
                                touched += 1;
                            }
                            Ok(_) => {}
                            Err(e) => log::warn!(
                                target: "session",
                                "跳过无法清洗的 raw 文件 {}: {e:#}",
                                path.display()
                            ),
                        }
                    }
                }
            }
        }

        if removed > 0 {
            log::info!(
                target: "session",
                "旧会话工具链清理完成：删除 {removed} 条不完整/孤立消息，涉及 {touched} 个文件"
            );
        }

        std::fs::create_dir_all(self.session_path())
            .with_context(|| "创建会话目录失败")?;
        std::fs::write(&marker, format!("removed={removed}\n"))
            .with_context(|| format!("写入清洗标记失败: {}", marker.display()))?;
        Ok(removed)
    }

    /// 清洗单个 raw 块：兼容信封结构与旧版裸数组，保留其余字段。
    fn cleanup_raw_file(&self, path: &Path) -> Result<usize> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取 raw 文件失败: {}", path.display()))?;
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("解析 raw 文件失败: {}", path.display()))?;

        let mut messages: Vec<Message> = if value.is_array() {
            serde_json::from_value(value.clone())
                .with_context(|| format!("解析 raw 消息失败: {}", path.display()))?
        } else {
            let Some(items) = value.get("messages") else {
                return Ok(0);
            };
            serde_json::from_value(items.clone())
                .with_context(|| format!("解析 raw 消息失败: {}", path.display()))?
        };

        let removed = super::migrate::sanitize_tool_chains(&mut messages);
        if removed == 0 {
            return Ok(0);
        }

        let serialized = serde_json::to_value(&messages).context("序列化 raw 消息失败")?;
        if value.is_array() {
            value = serialized;
        } else {
            value["messages"] = serialized;
        }
        let text = serde_json::to_string_pretty(&value).context("序列化 raw 文件失败")?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)
            .with_context(|| format!("写入 raw 临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("替换 raw 文件失败: {}", path.display()))?;
        Ok(removed)
    }

    /// 从 session_id 提取时间戳（去掉 {channel}_ 前缀）
    fn doc_timestamp(&self, session_id: &str, channel: &str) -> String {
        session_id
            .strip_prefix(&format!("{channel}_"))
            .unwrap_or(session_id)
            .to_string()
    }

    /// 标题为「初始时间戳占位」或空时，用首条 user 消息截断生成（归档前调用，因 compress 会清空 messages）
    fn ensure_title(&self, channel: &str) -> Result<()> {
        let mut data = self.read_current(channel)?;
        if data.id.is_empty() {
            return Ok(());
        }
        // 标题为空或等于初始时间戳占位（用户未手动改名）时，才用首条消息覆盖
        let default_ts = self.doc_timestamp(&data.id, channel);
        if !data.title.is_empty() && data.title != default_ts {
            return Ok(());
        }
        let first = data.messages.iter().find_map(|m| match m {
            Message::User { content, .. } => Some(content.clone()),
            _ => None,
        });
        if let Some(content) = first {
            data.title = truncate_title(&content);
            self.write_current(channel, &data)?;
        }
        Ok(())
    }

    // ============ 压缩编排 ============

    /// 会话压缩：把当前全部未压缩消息压成结构化摘要。
    ///
    /// 写侧保证 messages 一定是完整轮次，因此这里不再选择保留边界；
    /// 压缩成功后 messages 清空，锚点清零，下一次请求直接发送并由 provider 重新锚定。
    /// 返回是否实际写入压缩块；空会话或摘要为空时返回 false。
    async fn compress(&self, channel: &str) -> Result<bool> {
        let mut data = self.read_current(channel)?;
        if data.messages.is_empty() {
            log::debug!(target: "session", "当前会话为空，无需压缩。");
            return Ok(false);
        }

        let cfg = self.config.get();

        // 1. 全部未压缩消息 → 干净对话转录（不是存储 JSON）
        let to_compress: Vec<Message> = data.messages.clone();
        let transcript = build_transcript(&to_compress);
        let (system, user) = build_compress_prompt(&transcript);

        // 2. 调用 LLM 压缩（动态读最新配置构建 client）
        let model = cfg
            .active_model_config()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("未配置活跃模型（active_model 无效）"))?;
        let client = self.llm.client(model, cfg.llm.clone());
        let result: CompressResult = client.compress(&system, &user).await?;
        // 空结果不能清空原消息，否则会话内容会静默丢失
        if result.summary.is_empty()
            && result.facts.is_empty()
            && result.decisions.is_empty()
            && result.open_tasks.is_empty()
            && result.tool_outcomes.is_empty()
        {
            log::warn!(target: "session", "压缩结果为空，保留原消息不落盘。");
            return Ok(false);
        }

        // 3. 组装结构化压缩块
        let round = data.compress_round + 1;
        let chunk = CompressedChunk {
            chunk: round.to_string(),
            summary: result.summary,
            keywords: result.keywords,
            merged_from_chunks: Vec::new(),
            facts: result.facts,
            decisions: result.decisions,
            open_tasks: result.open_tasks,
            tool_outcomes: result.tool_outcomes,
            source_message_ids: Vec::new(),
            time_range: message_time_range(&to_compress).or(result.time_range),
            token_count: Some(estimate_text_tokens(&transcript)),
        };
        data.compress_round = round;
        data.compressed.push(chunk.clone());

        // 4. 写 raw（全部被压缩消息，带元信息）
        self.write_raw_chunk(&data.id, round, &to_compress)?;

        // 5. 全部消息已进入摘要与 raw，清空当前消息；锚点失效，等待 provider 重新锚定
        data.messages.clear();
        data.stats.last_input_tokens = 0;
        data.stats.last_output_tokens = 0;
        self.write_current(channel, &data)?;

        // 6. 同步向量库
        let index_text = chunk.index_text();
        if !index_text.is_empty() {
            let doc_id = format!("{}_chunk_{round}", data.id);
            let timestamp = self.doc_timestamp(&data.id, channel);
            self.vector
                .insert(&doc_id, &index_text, &chunk.keywords, &timestamp)?;
        }

        log::info!(
            target: "session",
            "当前会话已压缩全部消息：块 {round}，{} 条消息 → 摘要，现有 {} 个压缩记录",
            to_compress.len(),
            data.compressed.len()
        );

        // 7. 滚动摘要检查
        self.progressive_summarize(channel, &mut data).await?;

        Ok(true)
    }

    /// 滚动摘要：compressed 超 max_chunks 时，把旧滚动状态 + 最旧块合并成新的滚动状态。
    ///
    /// `super_compressed` 只有一层，不再对摘要反复做二级嵌套。
    async fn progressive_summarize(&self, channel: &str, data: &mut SessionData) -> Result<()> {
        let max_chunks = self.config.get().session_memory.max_chunks;
        // max_chunks < 2 时 merge_batch < 1，无意义，跳过
        if max_chunks < 2 || data.compressed.len() <= max_chunks {
            return Ok(());
        }
        let merge_batch = max_chunks / 2;

        let old_rolling = data.super_compressed.clone();
        let old_chunks: Vec<CompressedChunk> = data.compressed[..merge_batch].to_vec();
        let recent_chunks: Vec<CompressedChunk> = data.compressed[merge_batch..].to_vec();

        // 输入：旧滚动状态 + 新块的结构化摘要（不是嵌套摘要数组）
        let payload = json!({
            "old_rolling": old_rolling.as_ref().map(chunk_payload),
            "new_chunks": old_chunks.iter().map(chunk_payload).collect::<Vec<_>>(),
        });
        let (system, user) = build_rolling_prompt(&payload.to_string());

        let cfg = self.config.get();
        let model = cfg
            .active_model_config()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("未配置活跃模型（active_model 无效）"))?;
        let client = self.llm.client(model, cfg.llm.clone());
        let merged_result: CompressResult = match client.compress(&system, &user).await {
            Ok(r) => r,
            Err(e) => {
                log::warn!(target: "session", "滚动摘要调用失败: {e}");
                return Ok(());
            }
        };

        // 来源块：旧滚动覆盖的块 + 本次合并的块
        let mut merged_from: Vec<String> = Vec::new();
        if let Some(sc) = &old_rolling {
            if sc.merged_from_chunks.is_empty() {
                merged_from.push(sc.chunk.clone());
            } else {
                merged_from.extend(sc.merged_from_chunks.iter().cloned());
            }
        }
        merged_from.extend(old_chunks.iter().map(|c| c.chunk.clone()));
        dedup_keep_order(&mut merged_from);

        // 覆盖范围包含旧滚动摘要代表的更早块，避免编号只显示本次合并的尾部
        let nums: Vec<u64> = merged_from
            .iter()
            .flat_map(|c| extract_chunk_nums(c))
            .collect();
        let start = nums.iter().min().copied().unwrap_or(1);
        let end = nums.iter().max().copied().unwrap_or(merge_batch as u64);
        let merged_num = format!("merged_{start}_{end}");

        let merged_chunk = merge_rolling_chunk(
            merged_num.clone(),
            old_rolling.as_ref(),
            &old_chunks,
            merged_result,
            merged_from,
        );

        // 更新数据 + 落盘
        data.super_compressed = Some(merged_chunk.clone());
        data.compressed = recent_chunks;
        self.write_current(channel, data)?;

        // 同步向量库：删旧块（含旧滚动摘要），插新滚动摘要
        let session_id = data.id.clone();
        let mut delete_ids: Vec<String> = old_chunks
            .iter()
            .map(|c| format!("{session_id}_chunk_{}", c.chunk))
            .collect();
        if let Some(sc) = &old_rolling {
            delete_ids.push(format!("{session_id}_chunk_{}", sc.chunk));
        }
        let _ = self.vector.batch_delete(&delete_ids);

        let merged_doc_id = format!("{session_id}_chunk_{merged_num}");
        let merged_text = merged_chunk.index_text();
        if !merged_text.is_empty() {
            let timestamp = self.doc_timestamp(&session_id, channel);
            let _ = self.vector.insert(
                &merged_doc_id,
                &merged_text,
                &merged_chunk.keywords,
                &timestamp,
            );
        }

        // 写 raw（merged_summary）：保存本次滚动输入与输出，便于追溯
        let raw_dir = self.history_dir().join(&session_id).join("raw");
        if std::fs::create_dir_all(&raw_dir).is_ok() {
            let raw = json!({
                "type": "merged_summary",
                "created_at": chrono::Local::now().to_rfc3339(),
                "merged_from_chunks": merged_chunk.merged_from_chunks,
                "summary": merged_chunk.summary,
                "keywords": merged_chunk.keywords,
                "facts": merged_chunk.facts,
                "decisions": merged_chunk.decisions,
                "open_tasks": merged_chunk.open_tasks,
                "tool_outcomes": merged_chunk.tool_outcomes,
                "time_range": merged_chunk.time_range,
            });
            if let Ok(content) = serde_json::to_string_pretty(&raw) {
                let _ = std::fs::write(raw_dir.join(format!("{merged_num}.json")), content);
            }
        }

        log::info!(target: "session",
            "[RollingSummary] 旧滚动状态 + {} 个块 → {merged_num}（覆盖 {start}~{end}，compressed 剩余 {} 块）",
            old_chunks.len(),
            data.compressed.len()
        );
        Ok(())
    }

    /// 写 raw/{chunk}.json：带会话、块号、时间与 artifact 引用的信封结构
    fn write_raw_chunk(&self, session_id: &str, chunk: usize, messages: &[Message]) -> Result<()> {
        let raw_dir = self.history_dir().join(session_id).join("raw");
        std::fs::create_dir_all(&raw_dir)
            .with_context(|| format!("创建 raw 目录失败: {}", raw_dir.display()))?;
        let mut artifact_ids: Vec<String> = messages
            .iter()
            .filter_map(|m| match m {
                Message::Tool {
                    artifact: Some(a), ..
                } => Some(a.id.clone()),
                _ => None,
            })
            .collect();
        dedup_keep_order(&mut artifact_ids);

        let raw = RawChunkFile {
            kind: "messages".to_string(),
            session_id: session_id.to_string(),
            chunk: chunk.to_string(),
            created_at: chrono::Local::now().to_rfc3339(),
            messages: messages.to_vec(),
            artifact_ids,
        };
        let content = serde_json::to_string_pretty(&raw).context("序列化 raw 失败")?;
        std::fs::write(raw_dir.join(format!("{chunk}.json")), content)
            .with_context(|| "写入 raw 文件失败")?;
        Ok(())
    }

    // ============ 原始块读取 ============

    /// 按块号回溯读取压缩块原始消息：before_chunk=None 读最新块，否则读「小于 before 的最大块」。
    /// 返回 (块号, 消息, 是否还有更早块)。忽略 merged_ 摘要文件（其 file_stem 非纯数字，parse 失败即跳过）。
    fn load_history_chunk(
        &self,
        id: &str,
        before_chunk: Option<u64>,
    ) -> Result<(Option<u64>, Vec<Message>, bool)> {
        let raw_dir = self.history_dir().join(id).join("raw");
        if !raw_dir.is_dir() {
            return Ok((None, Vec::new(), false));
        }
        // 收集所有纯数字块号（普通压缩块），merged_ 摘要文件自然被忽略
        let mut nums: Vec<u64> = Vec::new();
        for entry in std::fs::read_dir(&raw_dir).with_context(|| "读取 raw 目录失败")? {
            let path = entry.context("读取 raw 条目失败")?.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(n) = stem.parse::<u64>() {
                nums.push(n);
            }
        }
        if nums.is_empty() {
            return Ok((None, Vec::new(), false));
        }
        nums.sort_unstable();

        let target = match before_chunk {
            Some(b) => nums.iter().rev().find(|&&n| n < b).copied(),
            None => nums.last().copied(),
        };
        let Some(n) = target else {
            return Ok((None, Vec::new(), false));
        };

        let file = raw_dir.join(format!("{n}.json"));
        let content = std::fs::read_to_string(&file)
            .with_context(|| format!("读取 raw 文件失败: {}", file.display()))?;
        let messages = serde_json::from_str::<RawChunkContent>(&content)
            .with_context(|| format!("解析 raw 文件失败: {}", file.display()))?
            .into_messages();
        let has_earlier = nums.first().copied().map(|min| min < n).unwrap_or(false);
        Ok((Some(n), messages, has_earlier))
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

    /// raw 目录里所有普通压缩块编号（忽略 merged_ 摘要），升序
    fn raw_chunk_nums(&self, id: &str) -> Result<Vec<u64>> {
        let raw_dir = self.history_dir().join(id).join("raw");
        if !raw_dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut nums = Vec::new();
        for entry in std::fs::read_dir(&raw_dir).with_context(|| "读取 raw 目录失败")? {
            let path = entry.context("读取 raw 条目失败")?.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Ok(n) = stem.parse::<u64>() {
                nums.push(n);
            }
        }
        nums.sort_unstable();
        Ok(nums)
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

    // ============ 重建索引 ============

    /// 重建向量索引：清空 + 先收集候选文件 → 逐个处理并上报进度
    pub fn rebuild_index(
        &self,
        on_progress: &(dyn Fn(RebuildProgress) + Send + Sync),
    ) -> Result<RebuildStats> {
        let mut stats = self.vector.rebuild()?;

        // ① 收集待处理 session 文件（归档 history/*/index.json + 活跃 current/*.json）
        let mut files: Vec<(PathBuf, RebuildSource)> = Vec::new();
        let history = self.history_dir();
        if history.is_dir() {
            for entry in std::fs::read_dir(&history).with_context(|| "读取 history 目录失败")? {
                let session_dir = entry.context("读取 history 条目失败")?.path();
                if !session_dir.is_dir() {
                    continue;
                }
                let index_file = session_dir.join("index.json");
                if index_file.is_file() {
                    files.push((index_file, RebuildSource::Archived));
                }
            }
        }
        let current = self.session_path().join("current");
        if current.is_dir() {
            for entry in std::fs::read_dir(&current).with_context(|| "读取 current 目录失败")? {
                let path = entry.context("读取 current 条目失败")?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    files.push((path, RebuildSource::Current));
                }
            }
        }

        // ② 逐个处理：先上报进度，再读文件 + 入库
        let total = files.len();
        for (i, (path, source)) in files.into_iter().enumerate() {
            on_progress(RebuildProgress {
                processed: i + 1,
                total,
                current: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            });

            match self.read_session_file(&path) {
                Ok(data) => {
                    // current 源仅活跃会话入库；archived 源仅归档会话入库（跳过 active 占位 index，避免重复）
                    let valid = match source {
                        RebuildSource::Current => {
                            data.status == SessionStatus::Active && !data.id.is_empty()
                        }
                        RebuildSource::Archived => data.status == SessionStatus::Archived,
                    };
                    if !valid {
                        continue;
                    }
                    let n = self.reindex_from_data(&data)?;
                    if n > 0 {
                        match source {
                            RebuildSource::Archived => stats.archived_sessions += 1,
                            RebuildSource::Current => stats.active_sessions += 1,
                        }
                        stats.total_chunks += n;
                    }
                }
                Err(e) => stats.errors.push(format!("读取失败: {}: {e}", path.display())),
            }
        }

        Ok(stats)
    }

    fn read_session_file(&self, path: &Path) -> Result<SessionData> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取会话文件失败: {}", path.display()))?;
        serde_json::from_str(&content).with_context(|| format!("解析会话文件失败: {}", path.display()))
    }

    /// 从会话数据提取 compressed + super_compressed 并插入向量库，返回入库块数
    fn reindex_from_data(&self, data: &SessionData) -> Result<usize> {
        let channel = data.id.split_once('_').map(|(c, _)| c).unwrap_or("");
        let timestamp = data
            .id
            .strip_prefix(&format!("{channel}_"))
            .unwrap_or(&data.id)
            .to_string();
        let mut count = 0;

        for chunk in &data.compressed {
            let doc_id = format!("{}_chunk_{}", data.id, chunk.chunk);
            let text = chunk.index_text();
            if !text.is_empty() {
                self.vector.insert(&doc_id, &text, &chunk.keywords, &timestamp)?;
                count += 1;
            }
        }

        if let Some(sc) = &data.super_compressed {
            let doc_id = format!("{}_chunk_{}", data.id, sc.chunk);
            let text = sc.index_text();
            if !text.is_empty() {
                self.vector.insert(&doc_id, &text, &sc.keywords, &timestamp)?;
                count += 1;
            }
        }

        Ok(count)
    }
}

#[async_trait]
impl SessionStore for FileSessionStore {
    fn init_session(&self, channel: &str) -> Result<()> {
        let data = self.read_current(channel)?;
        if data.status == SessionStatus::Active && !data.id.is_empty() {
            log::debug!(target: "session", "已识别到上次会话，继续上次对话。");
            return Ok(());
        }
        self.create_active_session(channel)?;
        Ok(())
    }

    async fn create_session(&self, channel: &str) -> Result<SessionData> {
        // ① 归档当前（若非空；空会话会被跳过）
        let current = self.read_current(channel)?;
        if current.status == SessionStatus::Active && !current.id.is_empty() {
            self.save_current_session(channel).await?;
        }
        // ② 新建 active 会话（写 current + history 初始存档）
        self.create_active_session(channel)
    }

    fn get_current_session(&self, channel: &str) -> Result<SessionData> {
        self.read_current(channel)
    }

    fn get_context_usage(&self, channel: &str) -> Result<ContextUsage> {
        let data = self.read_current(channel)?;
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

    fn update_current_session(&self, channel: &str, chat_history: &[Message]) -> Result<()> {
        let mut data = self.ensure_active(channel)?;
        data.messages.extend_from_slice(chat_history);
        self.write_current(channel, &data)
    }

    fn record_request_usage(
        &self,
        channel: &str,
        model: &str,
        usage: &crate::llm::TokenUsage,
    ) -> Result<()> {
        let mut data = self.read_current(channel)?;
        if data.id.is_empty() {
            return Ok(());
        }

        data.stats = SessionStats {
            last_input_tokens: usage.input_tokens as usize,
            last_output_tokens: usage.output_tokens as usize,
            last_model: model.to_string(),
            last_request_at: chrono::Local::now().to_rfc3339(),
        };
        self.write_current(channel, &data)
    }

    fn set_current_title(&self, channel: &str, title: &str) -> Result<()> {
        let mut data = self.ensure_active(channel)?;
        if data.title == title {
            return Ok(());
        }
        data.title = title.to_string();
        self.write_current(channel, &data)
    }

    async fn auto_compress_check(
        &self,
        channel: &str,
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
            self.compress(channel).await
        } else {
            Ok(false)
        }
    }

    async fn compress_session(&self, channel: &str) -> Result<bool> {
        // 空会话无需压缩（compress 内部也会短路）
        self.compress(channel).await
    }

    async fn save_current_session(&self, channel: &str) -> Result<()> {
        let mut data = self.read_current(channel)?;
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
            self.write_current(channel, &SessionData::empty())?;
            log::debug!(target: "session", "当前会话为空，跳过归档。");
            return Ok(());
        }

        self.ensure_title(channel)?;
        // 归档时把全部未压缩消息压成摘要；压缩失败时保留原文随 index 一起存档
        let _ = self.compress(channel).await?;
        data = self.read_current(channel)?;
        data.status = SessionStatus::Archived;
        self.write_session_index(&data)?;
        self.write_current(channel, &SessionData::empty())
    }

    fn rebuild_index(
        &self,
        on_progress: &(dyn Fn(RebuildProgress) + Send + Sync),
    ) -> Result<RebuildStats> {
        FileSessionStore::rebuild_index(self, on_progress)
    }

    fn list_sessions(&self, channel: &str) -> Result<Vec<SessionMeta>> {
        let mut out = Vec::new();
        let history = self.history_dir();
        if history.is_dir() {
            let prefix = format!("{channel}_");
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
                        // 统一遍历 history：active + archived 都在此存档，仅过滤当前 channel
                        if data.id.starts_with(&prefix) {
                            out.push(meta_from(&data));
                        }
                    }
                }
            }
        }
        // 按 id 倒序（id 字典序 = 时间序，最新在前；前端再自行置顶 active）
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    async fn switch_session(&self, channel: &str, id: &str) -> Result<SessionData> {
        // ① 目标会话必须存在（archived index）
        let src = self.history_dir().join(id).join("index.json");
        if !src.is_file() {
            return Err(anyhow::anyhow!("会话 '{id}' 不存在"));
        }

        // 已是当前会话则直接返回（防御性，前端已拦截）
        let current = self.read_current(channel)?;
        if current.id == id {
            return Ok(current);
        }

        let mut target = self.read_session_file(&src)?;

        // ② 归档当前（若非空）
        if current.status == SessionStatus::Active && !current.id.is_empty() {
            self.save_current_session(channel).await?;
        }

        // ③ 目标 status 改 active：写回 history index（存档备份，可容错回档）+ 写 current（进行中）
        target.status = SessionStatus::Active;
        self.write_session_index(&target)?;
        self.write_current(channel, &target)?;
        Ok(target)
    }

    fn load_session_history(
        &self,
        id: &str,
        before_chunk: Option<u64>,
    ) -> Result<LoadHistoryResult> {
        let (chunk, messages, has_earlier) = self.load_history_chunk(id, before_chunk)?;
        Ok(LoadHistoryResult { chunk, messages, has_earlier })
    }

    fn list_all_sessions(&self) -> Result<Vec<SessionMeta>> {
        self.all_session_metas()
    }

    fn read_session_raw_messages(&self, id: &str) -> Result<Vec<Message>> {
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
        channel: &str,
        artifact_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<ArtifactRead> {
        let data = self.read_current(channel)?;
        if data.id.is_empty() {
            anyhow::bail!("当前没有进行中的会话");
        }
        artifact::read(&self.session_path(), &data.id, artifact_id, offset, limit)
    }

    fn delete_session(&self, id: &str) -> Result<()> {
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

    fn rename_session(&self, channel: &str, id: &str, title: &str) -> Result<()> {
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
        let current = self.read_current(channel)?;
        if current.id == id {
            let mut c = current;
            c.title = title;
            self.write_current(channel, &c)?;
        }
        Ok(())
    }
}

/// 重建索引时的文件来源（决定统计到 archived 还是 active）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebuildSource {
    Archived,
    Current,
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

/// 压缩块 → 滚动摘要提示词的结构化载荷
fn chunk_payload(chunk: &CompressedChunk) -> serde_json::Value {
    json!({
        "chunk": chunk.chunk,
        "summary": chunk.summary,
        "keywords": chunk.keywords,
        "facts": chunk.facts,
        "decisions": chunk.decisions,
        "open_tasks": chunk.open_tasks,
        "tool_outcomes": chunk.tool_outcomes,
        "time_range": chunk.time_range,
    })
}

/// 组装滚动摘要块：模型输出优先，缺项时回退到旧滚动状态 + 被合并块的字段拼接。
fn merge_rolling_chunk(
    chunk: String,
    old_rolling: Option<&CompressedChunk>,
    old_chunks: &[CompressedChunk],
    result: CompressResult,
    merged_from: Vec<String>,
) -> CompressedChunk {
    let sources: Vec<&CompressedChunk> = old_rolling.into_iter().chain(old_chunks).collect();

    let fallback = |pick: fn(&CompressedChunk) -> &Vec<String>, limit: usize| -> Vec<String> {
        let mut out: Vec<String> = sources
            .iter()
            .flat_map(|c| pick(c).iter().cloned())
            .collect();
        dedup_keep_order(&mut out);
        out.truncate(limit);
        out
    };

    let summary = if result.summary.is_empty() {
        fallback(|c| &c.summary, 20)
    } else {
        result.summary
    };
    let keywords = if result.keywords.is_empty() {
        fallback(|c| &c.keywords, 20)
    } else {
        result.keywords
    };
    let facts = if result.facts.is_empty() {
        fallback(|c| &c.facts, 20)
    } else {
        result.facts
    };
    let decisions = if result.decisions.is_empty() {
        fallback(|c| &c.decisions, 20)
    } else {
        result.decisions
    };
    let open_tasks = if result.open_tasks.is_empty() {
        fallback(|c| &c.open_tasks, 10)
    } else {
        result.open_tasks
    };
    let tool_outcomes = if result.tool_outcomes.is_empty() {
        fallback(|c| &c.tool_outcomes, 10)
    } else {
        result.tool_outcomes
    };
    let time_range = result
        .time_range
        .or_else(|| old_rolling.and_then(|c| c.time_range.clone()));
    let token_count = Some(sources.iter().filter_map(|c| c.token_count).sum::<usize>());

    CompressedChunk {
        chunk,
        summary,
        keywords,
        merged_from_chunks: merged_from,
        facts,
        decisions,
        open_tasks,
        tool_outcomes,
        source_message_ids: Vec::new(),
        time_range,
        token_count,
    }
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
    let ts = id.splitn(2, '_').nth(1).unwrap_or(id);
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d-%H_%M_%S")
        .ok()
        .and_then(|naive| naive.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

/// 从会话数据构建列表元信息（标题空则回退时间戳）
fn meta_from(data: &SessionData) -> SessionMeta {
    let title = if data.title.is_empty() {
        data.id
            .splitn(2, '_')
            .nth(1)
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
