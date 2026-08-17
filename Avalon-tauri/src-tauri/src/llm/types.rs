// LLM 模块数据结构定义
//
// 整体对齐 OpenAI 兼容 API 的返回结构，并承载 ReAct 循环所需的
// 结构化字段（next / tool_call / summary 等）。

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// 统一 token 用量（对齐 OpenAI usage 字段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

/// 对话层控制意图：继续对话 or 进入动作层
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NextAction {
    Stop,
    Action,
}

/// 对话层结果（结构化，替代 Python 的 dict + 字符串字段）
#[derive(Debug, Clone, Serialize)]
pub struct ChatResult {
    pub thought: String,
    pub message: String,
    pub next: NextAction,
    #[serde(default)]
    pub action_target: Option<String>,
    #[serde(default)]
    pub usage: TokenUsage,
}

/// 动作层步骤意图
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStep {
    ToolCall,
    SubAnalysis,
    Finished,
}

// ActionStep 自定义反序列化：非法值兜底为 Finished，
// 避免模型输出异常 next 时整体解析失败。
impl<'de> Deserialize<'de> for ActionStep {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "tool_call" => ActionStep::ToolCall,
            "sub_analysis" => ActionStep::SubAnalysis,
            _ => ActionStep::Finished,
        })
    }
}

/// 工具调用描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// 动作层结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub analysis: String,
    pub next: ActionStep,
    #[serde(default)]
    pub tool_call: Option<ToolCall>,
    #[serde(default)]
    pub sub_analysis: Option<String>,
    #[serde(default)]
    pub usage: TokenUsage,
}

/// 压缩层结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressResult {
    pub summary: Vec<String>,
    pub keywords: Vec<String>,
    #[serde(default)]
    pub usage: TokenUsage,
}

/// 流式事件（通过 Tauri Channel 推给前端）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// 思考增量（前端思考块逐字渲染）
    ThoughtDelta { delta: String },
    /// 正文增量（前端逐字渲染）
    MessageDelta { delta: String },
    /// 完成：完整结果（驱动循环、落库）
    Done { result: ChatResult },
}
