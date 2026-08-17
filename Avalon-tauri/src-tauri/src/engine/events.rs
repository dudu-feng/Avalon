// Engine 事件定义
//
// 前端经 Tauri Channel 订阅的完整事件契约，serde(tag = "type") 让前端 switch 无遗漏。
// 与 Python react_emitter 的 9 个事件一一对应；ActionStep / ChatResult / TokenUsage 复用 llm 层类型。

#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;

use crate::llm::{ActionStep, ChatResult, TokenUsage};

/// 引擎事件（序列化为 {"type": "...", ...} 推给前端）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// 思考增量（前端思考块逐字渲染）
    ThoughtDelta { delta: String },
    /// 正文增量（前端逐字渲染）
    MessageDelta { delta: String },
    /// 进入动作层（target = 动作目标描述）
    ActionStart { target: String },
    /// 动作层步骤意图
    ActionStep { analysis: String, next: ActionStep },
    /// 工具调用
    ActionToolCall { tool_name: String, arguments: Value },
    /// 工具执行结果
    ActionToolResult { tool_name: String, success: bool, result: String },
    /// 子步骤分析
    ActionSubAnalysis { analysis: String, sub_analysis: String },
    /// 动作层结束
    ActionFinished { analysis: String, token_usage: TokenUsage },
    /// 整体结束（结果驱动前端落库展示）
    Done { result: ChatResult },
    /// 非致命异常（致命错误走 Result 由 command 层转错误）
    Error { code: i32, message: String },
}
