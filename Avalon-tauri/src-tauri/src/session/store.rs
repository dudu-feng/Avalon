// FileSessionStore：会话文件 CRUD + 压缩 + 渐进式总结 + 限界上下文 + rebuild 编排
//
// 存储布局：current/{channel}.json（活跃）+ history/{id}/index.json（归档）+ history/{id}/raw/{chunk}.json（原始消息）。
// 依赖注入：config（动态读路径/阈值）+ llm（压缩时动态构建 client）+ vector（摘要入库）。

#![allow(dead_code)] // session 模块供未来 engine/tool 引用，当前无调用方，接入后移除

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::config::ConfigStore;
use crate::llm::{CompressResult, LlmState};
use crate::prompt::build_compress_prompt;
use crate::vector::{RebuildStats, VectorStore};

use super::types::*;
use super::{now_id_ts, SessionStore};

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
                eprintln!("[Session] 旧格式会话已忽略（{}）: {e}", path.display());
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

    /// 确保会话就绪：active 则复用，否则新建
    fn ensure_active(&self, channel: &str) -> Result<SessionData> {
        let data = self.read_current(channel)?;
        if data.status == SessionStatus::Active && !data.id.is_empty() {
            return Ok(data);
        }
        let fresh = SessionData::new_active(format!("{channel}_{}", now_id_ts()));
        self.write_current(channel, &fresh)?;
        Ok(fresh)
    }

    /// 从 session_id 提取时间戳（去掉 {channel}_ 前缀）
    fn doc_timestamp(&self, session_id: &str, channel: &str) -> String {
        session_id
            .strip_prefix(&format!("{channel}_"))
            .unwrap_or(session_id)
            .to_string()
    }

    // ============ 压缩编排 ============

    /// 会话压缩：把 session 压成摘要，追加 compressed，清空 session，同步向量库，触发渐进式总结
    async fn compress(&self, channel: &str) -> Result<()> {
        let mut data = self.read_current(channel)?;
        if data.messages.is_empty() {
            println!("当前会话为空，无需压缩。");
            return Ok(());
        }

        // 1. 组装压缩提示词（只压未压缩消息，不传已压缩的 compressed/super_compressed）
        let payload = serde_json::to_string(&data.messages).context("序列化待压缩会话失败")?;
        let (system, user) = build_compress_prompt(&payload);

        // 2. 调用 LLM 压缩（动态读最新配置构建 client）
        let cfg = self.config.get();
        let model = cfg.active_model_config().cloned()
            .ok_or_else(|| anyhow::anyhow!("未配置活跃模型（active_model 无效）"))?;
        let client = self.llm.client(model, cfg.llm.clone());
        let result: CompressResult = client.compress(&system, &user).await?;

        // 3. 组装压缩块
        let round = data.compress_round + 1;
        let chunk = CompressedChunk {
            chunk: round.to_string(),
            summary: result.summary,
            keywords: result.keywords,
            merged_from_chunks: Vec::new(),
        };
        data.compress_round = round;
        data.compressed.push(chunk.clone());

        // 4. 写 raw（压缩前的原始 session）
        let raw_dir = self.history_dir().join(&data.id).join("raw");
        std::fs::create_dir_all(&raw_dir)
            .with_context(|| format!("创建 raw 目录失败: {}", raw_dir.display()))?;
        let raw_content = serde_json::to_string_pretty(&data.messages).context("序列化 raw 失败")?;
        std::fs::write(raw_dir.join(format!("{round}.json")), raw_content)
            .with_context(|| "写入 raw 文件失败")?;

        // 5. 清空 messages + 落盘
        data.messages.clear();
        self.write_current(channel, &data)?;

        // 6. 同步向量库
        let doc_id = format!("{}_chunk_{round}", data.id);
        let timestamp = self.doc_timestamp(&data.id, channel);
        let summary_text = chunk.summary.join("\n");
        self.vector.insert(&doc_id, &summary_text, &chunk.keywords, &timestamp)?;

        println!("当前会话已压缩。{} 个压缩记录", data.compressed.len());

        // 7. 渐进式总结检查
        self.progressive_summarize(channel, &mut data).await?;

        Ok(())
    }

    /// 渐进式总结：compressed 超 max_chunks 时，把最旧块合并为 super_compressed
    async fn progressive_summarize(&self, channel: &str, data: &mut SessionData) -> Result<()> {
        let max_chunks = self.config.get().session_memory.max_chunks;
        // max_chunks < 2 时 merge_batch < 1，无意义，跳过
        if max_chunks < 2 || data.compressed.len() <= max_chunks {
            return Ok(());
        }
        let merge_batch = max_chunks / 2;

        // 构建待合并列表（超摘要存在时作为最旧逻辑块纳入）
        let (old_chunks, recent_chunks) = match &data.super_compressed {
            Some(sc) => {
                let mut old = vec![sc.clone()];
                old.extend(data.compressed[..merge_batch - 1].iter().cloned());
                (old, data.compressed[merge_batch - 1..].to_vec())
            }
            None => (
                data.compressed[..merge_batch].to_vec(),
                data.compressed[merge_batch..].to_vec(),
            ),
        };

        // 收集摘要 / 关键词 / 块编号范围
        let all_summaries: Vec<String> = old_chunks.iter().flat_map(|c| c.summary.clone()).collect();
        let all_keywords: Vec<String> = old_chunks.iter().flat_map(|c| c.keywords.clone()).collect();
        let nums: Vec<u64> = old_chunks.iter().flat_map(|c| extract_chunk_nums(&c.chunk)).collect();
        let start = nums.iter().min().copied().unwrap_or(1);
        let end = nums.iter().max().copied().unwrap_or(merge_batch as u64);
        let merged_num = format!("merged_{start}_{end}");

        // 二次压缩（把旧摘要当作 assistant 消息）
        let mock = json!({
            "messages": all_summaries
                .iter()
                .map(|s| json!({"role": "assistant", "content": s}))
                .collect::<Vec<_>>()
        });
        let (system, user) = build_compress_prompt(&mock.to_string());
        let cfg = self.config.get();
        let model = cfg.active_model_config().cloned()
            .ok_or_else(|| anyhow::anyhow!("未配置活跃模型（active_model 无效）"))?;
        let client = self.llm.client(model, cfg.llm.clone());
        let merged_result: CompressResult = match client.compress(&system, &user).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[ProgressiveSummarize] 合并压缩调用失败: {e}");
                return Ok(());
            }
        };

        let merged_chunk = CompressedChunk {
            chunk: merged_num.clone(),
            summary: if merged_result.summary.is_empty() {
                all_summaries.iter().take(3).cloned().collect()
            } else {
                merged_result.summary
            },
            keywords: if merged_result.keywords.is_empty() {
                let mut k: Vec<String> = all_keywords.clone();
                k.sort();
                k.dedup();
                k.truncate(10);
                k
            } else {
                merged_result.keywords
            },
            merged_from_chunks: old_chunks.iter().map(|c| c.chunk.clone()).collect(),
        };

        // 更新数据 + 落盘
        data.super_compressed = Some(merged_chunk.clone());
        data.compressed = recent_chunks;
        self.write_current(channel, data)?;

        // 同步向量库：删旧块（含旧超摘要），插超级摘要
        let session_id = data.id.clone();
        let delete_ids: Vec<String> = old_chunks
            .iter()
            .map(|c| format!("{session_id}_chunk_{}", c.chunk))
            .collect();
        let _ = self.vector.batch_delete(&delete_ids);

        let merged_doc_id = format!("{session_id}_chunk_{merged_num}");
        let merged_text = merged_chunk.summary.join("\n");
        let timestamp = self.doc_timestamp(&session_id, channel);
        let _ = self.vector.insert(&merged_doc_id, &merged_text, &merged_chunk.keywords, &timestamp);

        // 写 raw（merged_summary）
        let raw_dir = self.history_dir().join(&session_id).join("raw");
        if std::fs::create_dir_all(&raw_dir).is_ok() {
            let raw = json!({
                "type": "merged_summary",
                "merged_from_chunks": merged_chunk.merged_from_chunks,
                "summary": merged_chunk.summary,
                "keywords": merged_chunk.keywords,
            });
            if let Ok(content) = serde_json::to_string_pretty(&raw) {
                let _ = std::fs::write(raw_dir.join(format!("{merged_num}.json")), content);
            }
        }

        println!(
            "[ProgressiveSummarize] {} 个逻辑块 → 1 个超级摘要 {merged_num}（覆盖 {start}~{end}，compressed 剩余 {} 块）",
            old_chunks.len(),
            data.compressed.len()
        );
        Ok(())
    }

    // ============ 重建索引 ============

    /// 重建向量索引：清空 + 扫描 history/current + 重新入库
    pub fn rebuild_index(&self) -> Result<RebuildStats> {
        let mut stats = self.vector.rebuild()?;
        let history = self.history_dir();
        let current = self.session_path().join("current");

        // ① 扫描归档会话 history/*/index.json
        if history.is_dir() {
            for entry in std::fs::read_dir(&history).with_context(|| "读取 history 目录失败")? {
                let entry = entry.context("读取 history 条目失败")?;
                let session_dir = entry.path();
                if !session_dir.is_dir() {
                    continue;
                }
                let index_file = session_dir.join("index.json");
                if !index_file.is_file() {
                    continue;
                }
                match self.read_session_file(&index_file) {
                    Ok(data) => {
                        let n = self.reindex_from_data(&data)?;
                        if n > 0 {
                            stats.archived_sessions += 1;
                            stats.total_chunks += n;
                        }
                    }
                    Err(e) => stats.errors.push(format!("读取失败: {}: {e}", index_file.display())),
                }
            }
        }

        // ② 扫描活跃会话 current/*.json（status == active）
        if current.is_dir() {
            for entry in std::fs::read_dir(&current).with_context(|| "读取 current 目录失败")? {
                let entry = entry.context("读取 current 条目失败")?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                match self.read_session_file(&path) {
                    Ok(data) => {
                        if data.status == SessionStatus::Active && !data.id.is_empty() {
                            let n = self.reindex_from_data(&data)?;
                            if n > 0 {
                                stats.active_sessions += 1;
                                stats.total_chunks += n;
                            }
                        }
                    }
                    Err(e) => stats.errors.push(format!("读取失败: {}: {e}", path.display())),
                }
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
            let text = chunk.summary.join("\n");
            if !text.is_empty() {
                self.vector.insert(&doc_id, &text, &chunk.keywords, &timestamp)?;
                count += 1;
            }
        }

        if let Some(sc) = &data.super_compressed {
            let doc_id = format!("{}_chunk_{}", data.id, sc.chunk);
            let text = sc.summary.join("\n");
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
            println!("已识别到上次会话，继续上次对话。");
            return Ok(());
        }
        let fresh = SessionData::new_active(format!("{channel}_{}", now_id_ts()));
        self.write_current(channel, &fresh)
    }

    fn get_current_session(&self, channel: &str) -> Result<SessionData> {
        self.read_current(channel)
    }

    fn get_context_for_prompt(&self, channel: &str) -> Result<String> {
        let data = self.read_current(channel)?;
        let max = self.config.get().session_memory.context_chunks;
        let omitted = data.compressed.len().saturating_sub(max);
        let start = data.compressed.len() - max.min(data.compressed.len());
        let ctx = SessionContext {
            id: data.id,
            status: data.status,
            super_compressed: data.super_compressed,
            compressed: data.compressed[start..].to_vec(),
            messages: data.messages,
            older_chunks_omitted: (omitted > 0).then_some(omitted),
        };
        serde_json::to_string(&ctx).context("序列化会话上下文失败")
    }

    fn update_current_session(&self, channel: &str, chat_history: &[Message]) -> Result<()> {
        let mut data = self.ensure_active(channel)?;
        data.messages.extend_from_slice(chat_history);
        self.write_current(channel, &data)
    }

    async fn auto_compress_check(&self, channel: &str, chat_history: &[Message]) -> Result<bool> {
        let max_input = max_input_tokens(chat_history);
        let threshold = self.config.get().session_memory.compress_threshold;
        if max_input >= threshold {
            println!("[AutoCompress] 输入 token({max_input}) >= 阈值({threshold})，触发自动压缩...");
            self.compress(channel).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn save_current_session(&self, channel: &str) -> Result<()> {
        self.compress(channel).await?;
        let mut data = self.read_current(channel)?;
        if data.id.is_empty() {
            println!("当前无会话可归档。");
            return Ok(());
        }
        data.status = SessionStatus::Archived;
        let session_dir = self.history_dir().join(&data.id);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("创建归档目录失败: {}", session_dir.display()))?;
        let content = serde_json::to_string_pretty(&data).context("序列化归档数据失败")?;
        std::fs::write(session_dir.join("index.json"), content)
            .with_context(|| "写入 index.json 失败")?;
        self.write_current(channel, &SessionData::empty())
    }

    fn rebuild_index(&self) -> Result<RebuildStats> {
        FileSessionStore::rebuild_index(self)
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
