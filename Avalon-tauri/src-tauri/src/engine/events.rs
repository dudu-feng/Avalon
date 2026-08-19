// Engine 事件定义
//
// 前端经 Tauri Channel 订阅的完整事件契约，serde(tag = "type") 让前端 switch 无遗漏。
// 单模型 ReAct：思考（reasoning_content）/正文（content）/工具调用与结果/完成/错误。

#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;

use crate::llm::ChatResult;

/// 引擎事件（序列化为 {"type": "...", ...} 推给前端）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// 轮次边界：每轮大模型调用开始前发射，前端据此封口上一轮气泡、开新气泡
    RoundStart,
    /// 思考增量（reasoning_content，前端思考块逐字渲染）
    ThoughtDelta { delta: String },
    /// 正文增量（前端逐字渲染）
    MessageDelta { delta: String },
    /// 工具调用（发起，携带 id + 参数供前端展示）
    ToolCall { id: String, tool_name: String, arguments: Value },
    /// 工具执行结果（精简摘要）
    ToolResult { tool_name: String, success: bool, result: String },
    /// 整体结束（结果驱动前端落库展示）
    Done { result: ChatResult },
    /// 非致命异常（致命错误走 Result 由 command 层转错误）
    Error { code: i32, message: String },
}
