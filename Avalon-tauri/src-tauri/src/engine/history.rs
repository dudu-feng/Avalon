// 消息结构转换
//
// 把 llm 层结果（ChatResult）转成 session 层消息结构（Message），
// 供 session 持久化。engine 不定义类型，只做转换。

#![allow(dead_code)]

use serde_json::Value;

use crate::llm::ChatResult;
use crate::session::Message;

/// 消息时间戳：%Y-%m-%d-%H:%M:%S（冒号，对齐 Python get_current_time）
/// 与 session_id 的 %Y-%m-%d-%H_%M_%S（下划线，文件名安全）格式不同，分别定义。
pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H:%M:%S").to_string()
}

/// 用户消息（本轮输入）。meta 承载渠道来源，桌面端传 None
pub fn user_entry(content: &str, meta: Option<serde_json::Value>) -> Message {
    Message::User {
        time: now_str(),
        content: content.to_string(),
        meta,
    }
}

/// 助手消息（一轮 chat_stream 的结果：reasoning + content + tool_calls + usage）
pub fn assistant_entry(result: &ChatResult) -> Message {
    Message::Assistant {
        time: now_str(),
        content: result.message.clone(),
        // 空思考（模型未输出 reasoning）→ None，保持 Option 语义
        reasoning_content: (!result.thought.is_empty()).then(|| result.thought.clone()),
        // 空 tool_calls → None（最终正文轮）
        tool_calls: (!result.tool_calls.is_empty()).then(|| result.tool_calls.clone()),
        token_usage: result.usage.clone(),
        model: result.model.clone(),
    }
}

/// 工具执行结果消息（对齐 OpenAI tool 消息，content 存精简摘要，arguments 自包含）
pub fn tool_entry(
    tool_call_id: &str,
    name: &str,
    arguments: Value,
    success: bool,
    content: &str,
) -> Message {
    Message::Tool {
        time: now_str(),
        tool_call_id: tool_call_id.to_string(),
        name: name.to_string(),
        arguments,
        success,
        content: content.to_string(),
    }
}
