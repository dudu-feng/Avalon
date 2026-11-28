// 分块历史：raw/{chunk}.json 的写入、回溯读取与块号扫描。

use anyhow::{Context, Result};

use super::{dedup_keep_order, FileSessionStore, RawChunkContent, RawChunkFile};
use crate::session::types::Message;

impl FileSessionStore {
    /// 写 raw/{chunk}.json：带会话、块号、时间与 artifact 引用的信封结构
    pub(super) fn write_raw_chunk(
        &self,
        session_id: &str,
        chunk: usize,
        messages: &[Message],
    ) -> Result<()> {
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

    /// 按块号回溯读取压缩块原始消息：before_chunk=None 读最新块，否则读「小于 before 的最大块」。
    /// 返回 (块号, 消息, 是否还有更早块)。忽略 merged_ 摘要文件（其 file_stem 非纯数字，parse 失败即跳过）。
    pub(super) fn load_history_chunk(
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

    /// raw 目录里所有普通压缩块编号（忽略 merged_ 摘要），升序
    pub(super) fn raw_chunk_nums(&self, id: &str) -> Result<Vec<u64>> {
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
}
