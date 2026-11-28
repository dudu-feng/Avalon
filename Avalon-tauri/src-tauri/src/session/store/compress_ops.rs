// 压缩编排：调用 LLM 生成压缩块，并在块数超限时维护单层滚动摘要。
//
// 底层转录、提示词与结构化解析分别在 compress / prompt / llm 模块，本文件只负责读写时序。

use anyhow::Result;
use serde_json::json;

use super::{dedup_keep_order, extract_chunk_nums, FileSessionStore};
use crate::llm::CompressResult;
use crate::prompt::{build_compress_prompt, build_rolling_prompt};
use crate::session::compress::{build_transcript, message_time_range};
use crate::session::context::estimate_text_tokens;
use crate::session::types::{CompressedChunk, Message, SessionData};

impl FileSessionStore {
    /// 会话压缩：把当前全部未压缩消息压成结构化摘要。
    ///
    /// 写侧保证 messages 一定是完整轮次，因此这里不再选择保留边界；
    /// 压缩成功后 messages 清空，锚点清零，下一次请求直接发送并由 provider 重新锚定。
    /// 返回是否实际写入压缩块；空会话或摘要为空时返回 false。
    pub(super) async fn compress(&self) -> Result<bool> {
        let mut data = self.read_current()?;
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
        self.write_current(&data)?;

        // 6. 同步向量库（best-effort）：压缩块此时已落盘，向量写入失败只影响检索，
        //    不能反过来把调用方判成“压缩失败”——否则预检会拿旧上下文继续请求。
        let index_text = chunk.index_text();
        if !index_text.is_empty() {
            let doc_id = format!("{}_chunk_{round}", data.id);
            let timestamp = self.doc_timestamp(&data.id);
            if let Err(e) = self
                .vector
                .insert(&doc_id, &index_text, &chunk.keywords, &timestamp)
            {
                log::warn!(target: "session", "压缩块同步向量库失败（不影响压缩结果）: {e:#}");
            }
        }

        log::info!(
            target: "session",
            "当前会话已压缩全部消息：块 {round}，{} 条消息 → 摘要，现有 {} 个压缩记录",
            to_compress.len(),
            data.compressed.len()
        );

        // 7. 滚动摘要检查：失败只影响长期记忆，压缩本身已提交，不能向上报错
        if let Err(e) = self.progressive_summarize(&mut data).await {
            log::warn!(target: "session", "滚动摘要失败（压缩块已保存）: {e:#}");
        }

        Ok(true)
    }

    /// 滚动摘要：compressed 超 max_chunks 时，把旧滚动状态 + 最旧块合并成新的滚动状态。
    ///
    /// `super_compressed` 只有一层，不再对摘要反复做二级嵌套。
    async fn progressive_summarize(&self, data: &mut SessionData) -> Result<()> {
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
        self.write_current(data)?;

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
            let timestamp = self.doc_timestamp(&session_id);
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
