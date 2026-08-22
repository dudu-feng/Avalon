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
use crate::vector::{RebuildProgress, RebuildStats, VectorStore};

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
        let messages: Vec<Message> = serde_json::from_str(&content)
            .with_context(|| format!("解析 raw 文件失败: {}", file.display()))?;
        let has_earlier = nums.first().copied().map(|min| min < n).unwrap_or(false);
        Ok((Some(n), messages, has_earlier))
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
        let used_tokens = max_input_tokens(&data.messages);
        let threshold = self.config.get().session_memory.compress_threshold;
        Ok(ContextUsage { used_tokens, threshold })
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

    fn set_current_title(&self, channel: &str, title: &str) -> Result<()> {
        let mut data = self.ensure_active(channel)?;
        if data.title == title {
            return Ok(());
        }
        data.title = title.to_string();
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
        let mut data = self.read_current(channel)?;
        if data.id.is_empty() {
            println!("当前无会话可归档。");
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
            println!("当前会话为空，跳过归档。");
            return Ok(());
        }

        self.ensure_title(channel)?;
        self.compress(channel).await?;
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
