// 记忆检索工具实现
//
// search_session_memory：接入 vector::MemoryIndex 检索契约，供大模型查询历史会话记忆。
// 参数：query（必填）、mode（semantic/keyword/hybrid，缺省取配置 session_memory.search_mode）、
// topk（默认 5，上限 20）、time_range（可选，YYYY-MM-DD 或 YYYY-MM-DD,YYYY-MM-DD）。
// 检索走三模式 + 时间过滤，命中结果按分数降序格式化为文本返回。

use serde_json::Value;

use crate::config::SearchMode;
use crate::vector::{MemoryHit, MemoryIndex};

/// 默认检索条数（InMemoryStore.search_impl 内部还会 clamp 到 [1, 20]）
const DEFAULT_TOPK: u64 = 5;

/// search_session_memory 工具：查询历史会话记忆
/// `default_mode` 为 mode 参数缺省时的兜底，由调用方（ToolSet）从配置读入。
pub fn search_session_memory(
    args: &Value,
    memory: &dyn MemoryIndex,
    default_mode: SearchMode,
) -> String {
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return "参数错误: 缺少 query 或类型应为字符串".to_string();
    };
    let query = query.trim();
    if query.is_empty() {
        return "参数错误: query 不能为空".to_string();
    }

    let Some(mode) = parse_search_mode(args.get("mode").and_then(Value::as_str), default_mode) else {
        return "参数错误: mode 应为 semantic / keyword / hybrid".to_string();
    };

    let topk = args
        .get("topk")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TOPK) as usize;
    let time_range = args.get("time_range").and_then(Value::as_str).unwrap_or("");

    match memory.search(query, mode, topk, time_range) {
        Ok(hits) if hits.is_empty() => "未检索到相关记忆".to_string(),
        Ok(hits) => format_hits(&hits),
        Err(e) => format!("记忆检索失败: {e}"),
    }
}

/// 字符串 → 检索模式（大小写不敏感，缺省/空串用 default，非法值返回 None）
pub fn parse_search_mode(mode: Option<&str>, default: SearchMode) -> Option<SearchMode> {
    match mode.map(str::trim).filter(|s| !s.is_empty()) {
        None => Some(default),
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "semantic" => Some(SearchMode::Semantic),
            "keyword" => Some(SearchMode::Keyword),
            "hybrid" => Some(SearchMode::Hybrid),
            _ => None,
        },
    }
}

/// 命中结果 → 文本（按分数降序，含关键词/时间，供 LLM 引用具体记忆）
pub fn format_hits(hits: &[MemoryHit]) -> String {
    let mut out = String::new();
    for (i, h) in hits.iter().enumerate() {
        out.push_str(&format!("[{}] {}", i + 1, h.description));
        if !h.keywords.is_empty() {
            out.push_str(&format!(" （关键词: {}）", h.keywords.join("、")));
        }
        out.push_str(&format!(" [时间: {}]", h.timestamp));
        if i + 1 < hits.len() {
            out.push('\n');
        }
    }
    out
}
