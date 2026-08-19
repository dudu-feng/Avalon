// 消息结构转换
//
// 把 llm 层结果（ChatResult）转成 session 层消息结构（ChatMessage / ActionRecord），
// 供 session 持久化。engine 不定义类型，只做转换。

#![allow(dead_code)]

use crate::llm::{ChatResult, TokenUsage};
use crate::session::{ActionRecord, ChatMessage, MessageRole};

/// 消息时间戳：%Y-%m-%d-%H:%M:%S（冒号，对齐 Python get_current_time）
/// 与 session_id 的 %Y-%m-%d-%H_%M_%S（下划线，文件名安全）格式不同，分别定义。
pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H:%M:%S").to_string()
}

/// 用户消息（本轮输入）
pub fn user_entry(content: &str) -> ChatMessage {
    ChatMessage {
        role: MessageRole::User,
        time: now_str(),
        content: content.to_string(),
        thought: None,
        token_usage: TokenUsage::default(),
        action_history: None,
    }
}

/// 助手消息（对话层结果：thought（reasoning）+ message + usage）
pub fn assistant_entry(result: &ChatResult) -> ChatMessage {
    ChatMessage {
        role: MessageRole::Assistant,
        time: now_str(),
        content: result.message.clone(),
        // 空思考（模型未输出 reasoning）→ None，保持 Option 语义
        thought: (!result.thought.is_empty()).then(|| result.thought.clone()),
        token_usage: result.usage.clone(),
        action_history: None,
    }
}

/// 执行记录消息（工具调用后追加，content 精简为「工具名 + 结果摘要」，动作明细挂 action_history）
/// 精简是「注意力隔离」的核心：会话历史里不再堆积全量工具中间过程。
pub fn execution_record(records: Vec<ActionRecord>) -> ChatMessage {
    let mut summary = String::from("【执行记录】");
    for r in &records {
        let name = r
            .tool_call
            .as_ref()
            .map(|t| t.name.as_str())
            .unwrap_or("");
        let result = r.tool_result.as_deref().unwrap_or("");
        summary.push_str(&format!("\n- {name}: {result}"));
    }
    ChatMessage {
        role: MessageRole::Assistant,
        time: now_str(),
        content: summary,
        thought: None,
        token_usage: TokenUsage::default(),
        action_history: Some(records),
    }
}
