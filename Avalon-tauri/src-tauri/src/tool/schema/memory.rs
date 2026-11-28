// 记忆检索工具 schema。

use serde_json::json;

use super::ToolDef;

/// 记忆检索工具定义（仅注入 memory 后端时暴露）
pub(crate) fn memory_tool_def() -> ToolDef {
    ToolDef {
        name: "search_session_memory",
        description: "查询历史会话的压缩记忆摘要（不是原文）".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "检索关键词/问题"},
                "mode": {
                    "type": "string",
                    "enum": ["semantic", "keyword", "hybrid"],
                    "description": "检索模式（可选，缺省取配置）"
                },
                "topk": {"type": "integer", "description": "返回条数（可选，默认 5）"},
                "time_range": {
                    "type": "string",
                    "description": "时间范围 YYYY-MM-DD 或 YYYY-MM-DD,YYYY-MM-DD（可选）"
                }
            },
            "required": ["query"]
        }),
    }
}
