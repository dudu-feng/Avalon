// 向量索引重建：清空后扫描 current/history 会话文件并重新入库。

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::FileSessionStore;
use crate::session::types::{SessionData, SessionStatus};
use crate::vector::{RebuildProgress, RebuildStats};

/// 重建索引时的文件来源（决定统计到 archived 还是 active）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebuildSource {
    Archived,
    Current,
}

impl FileSessionStore {
    /// 重建向量索引：清空 + 先收集候选文件 → 逐个处理并上报进度
    pub(crate) fn rebuild_index(
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
                self.vector
                    .insert(&doc_id, &text, &chunk.keywords, &timestamp)?;
                count += 1;
            }
        }

        if let Some(sc) = &data.super_compressed {
            let doc_id = format!("{}_chunk_{}", data.id, sc.chunk);
            let text = sc.index_text();
            if !text.is_empty() {
                self.vector
                    .insert(&doc_id, &text, &sc.keywords, &timestamp)?;
                count += 1;
            }
        }

        Ok(count)
    }
}
