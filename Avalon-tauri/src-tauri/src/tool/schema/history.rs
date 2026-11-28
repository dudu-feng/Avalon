// 会话历史原文与工具结果回读 schema。

use serde_json::json;

use super::ToolDef;

/// 会话历史原文工具（仅主人）。与 search_session_memory 返回的压缩摘要区分开
pub(crate) fn history_tool_defs(is_owner: bool) -> Vec<ToolDef> {
    if !is_owner {
        return Vec::new();
    }
    vec![
        ToolDef {
            name: "list_session_history",
            description:
                "列出所有已保存的会话（跨渠道，含进行中与已归档），返回会话 id、状态和标题。\
                          这是查找历史原文的入口；只列出元信息，不返回消息正文。"
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "description": "最多返回几个会话（默认 50，最大 200）"}
                }
            }),
        },
        ToolDef {
            name: "read_session_history",
            description: "读取会话的原始消息（不是记忆检索的压缩摘要）。\
                          不传 session_id 时读取当前会话；用 list_session_history 可查历史会话 id。\
                          默认从最新消息往前分页，按时间正序返回。原文较长时用 offset 继续读取。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "可选，会话 id；不传读当前会话"},
                    "offset": {"type": "integer", "description": "从最新消息往前跳过多少条（默认 0）"},
                    "limit": {"type": "integer", "description": "本次最多返回多少条（默认 20，最大 100）"},
                    "include_tools": {"type": "boolean", "description": "是否显示工具执行结果（默认 false）"},
                    "include_reasoning": {"type": "boolean", "description": "是否显示助手思考内容（默认 false）"}
                }
            }),
        },
    ]
}

/// 工具结果回读定义。
///
/// 不按主人过滤：artifact 只属于当前会话，内容和用户在聊天界面里看到的一致；
/// 非主人也可能需要回看自己会话里的大体积工具输出。
pub(crate) fn read_tool_result_def() -> ToolDef {
    ToolDef {
        name: "read_tool_result",
        description: "读取当前会话里工具结果的完整内容。当工具消息末尾出现 \
                      [完整结果: artifact:xxx] 时，说明原输出较长，只展示了开头和结尾；\
                      需要中间内容时用本工具回读。分页用 offset 继续。"
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "artifact_id": {"type": "string", "description": "来自 [完整结果: artifact:xxx] 中的 id"},
                "offset": {"type": "integer", "description": "字符偏移，默认 0"},
                "limit": {"type": "integer", "description": "最多返回字符数，默认 4000，最大 20000"}
            },
            "required": ["artifact_id"]
        }),
    }
}
