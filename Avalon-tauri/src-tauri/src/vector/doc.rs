// 向量库数据结构定义

use serde::{Deserialize, Serialize};

/// 一条会话记忆记录（存储单元）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDoc {
    /// doc_id：{session_id}_chunk_{N} 或 {session_id}_chunk_merged_{N}
    pub id: String,
    /// 摘要文本（对应 Python description，keyword 检索的对象）
    pub description: String,
    /// 关键词列表（对应 Python keyWords，仅随结果返回，不参与检索）
    pub keywords: Vec<String>,
    /// 会话时间戳 "YYYY-MM-DD-HH_MM_SS"（定长，字符串比较 = 时间比较）
    pub timestamp: String,
    /// 512 维 L2 归一化向量（embedder 输出即归一化）
    pub vector: Vec<f32>,
}

/// 检索命中结果
#[derive(Debug, Clone, Serialize)]
pub struct MemoryHit {
    pub doc_id: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub timestamp: String,
    /// 相关度分数（semantic=余弦相似度 / keyword=BM25 / hybrid=RRF）
    pub score: f32,
}

/// 重建统计
#[derive(Debug, Clone, Serialize)]
pub struct RebuildStats {
    pub cleared: bool,
    pub archived_sessions: usize,
    pub active_sessions: usize,
    pub total_chunks: usize,
    pub errors: Vec<String>,
}

/// 重建进度事件（rebuild_memory_index 经 Channel 推送）
#[derive(Debug, Clone, Serialize)]
pub struct RebuildProgress {
    /// 已处理 session 数（1-based）
    pub processed: usize,
    /// 待处理 session 总数
    pub total: usize,
    /// 当前处理的文件名（供 UI 展示）
    pub current: String,
}

impl Default for RebuildStats {
    fn default() -> Self {
        Self {
            cleared: true,
            archived_sessions: 0,
            active_sessions: 0,
            total_chunks: 0,
            errors: Vec::new(),
        }
    }
}

/// 解析 doc_id：{session_id}_chunk_{N} / {session_id}_chunk_merged_{N} → (session_id, chunk)
/// 用 rfind 定位最后一个 `_chunk_`，避免 session_id 本身含该片段时误判。
pub fn parse_doc_id(doc_id: &str) -> (String, String) {
    const MARKER: &str = "_chunk_";
    match doc_id.rfind(MARKER) {
        Some(idx) => {
            let session_id = doc_id[..idx].to_string();
            let chunk = doc_id[idx + MARKER.len()..].to_string();
            (session_id, chunk)
        }
        None => (doc_id.to_string(), "0".to_string()),
    }
}
