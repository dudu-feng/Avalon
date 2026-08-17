// 消息结构转换
//
// 把 llm 层结果（ChatResult / ActionResult）转成 session 层消息结构
// （ChatMessage / ActionRecord），供 session 持久化。engine 不定义类型，只做转换。

#![allow(dead_code)]

use crate::llm::{ActionResult, ChatResult, TokenUsage};
use crate::session::{ActionRecord, ActionType, ChatMessage, MessageRole};

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

/// 助手消息（对话层结果：thought + message + usage）
pub fn assistant_entry(result: &ChatResult) -> ChatMessage {
    ChatMessage {
        role: MessageRole::Assistant,
        time: now_str(),
        content: result.message.clone(),
        // 空思考（模型未输出 thought 段）→ None，保持 Option 语义
        thought: (!result.thought.is_empty()).then(|| result.thought.clone()),
        token_usage: result.usage.clone(),
        action_history: None,
    }
}

/// 执行记录消息（动作层结束后追加，content 固定「执行记录」，动作明细挂 action_history）
pub fn execution_record(records: Vec<ActionRecord>) -> ChatMessage {
    ChatMessage {
        role: MessageRole::Assistant,
        time: now_str(),
        content: "【执行记录】".to_string(),
        thought: None,
        token_usage: TokenUsage::default(),
        action_history: Some(records),
    }
}

/// 动作步骤记录（每个 action 步骤产出一条，取代 Python 每步骤两条的冗余）
pub fn action_record(
    result: &ActionResult,
    kind: ActionType,
    tool_result: Option<String>,
) -> ActionRecord {
    ActionRecord {
        action_type: kind,
        time: now_str(),
        analysis: result.analysis.clone(),
        tool_call: result.tool_call.clone(),
        tool_result,
        sub_analysis: result.sub_analysis.clone(),
        token_usage: result.usage.clone(),
    }
}
