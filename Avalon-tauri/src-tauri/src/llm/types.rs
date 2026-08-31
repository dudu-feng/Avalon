// LLM 模块数据结构定义
//
// 整体对齐 OpenAI 兼容 API 的返回结构，并承载 ReAct 循环所需的
// 结构化字段（tool_calls 等）。单模型循环：一轮对话产出正文 + 思考 + 工具调用。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 统一 token 用量（对齐 OpenAI usage 字段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    /// 思考 token 数（completion_tokens_details.reasoning_tokens，DeepSeek reasoner 才有）
    #[serde(default)]
    pub reasoning_tokens: u32,
    /// 缓存命中的输入 token 数（prompt_tokens_details.cached_tokens）
    #[serde(default)]
    pub cached_tokens: u32,
}

/// 跨轮累加。
///
/// 用 AddAssign 而不是在调用处逐字段相加：字段列表只存在于这一个地方，
/// 以后再加字段不会因为漏改某个累加点而静默恒零 ——
/// reasoning_tokens 与 cached_tokens 就是这么丢了很久的。
impl std::ops::AddAssign<&TokenUsage> for TokenUsage {
    fn add_assign(&mut self, rhs: &TokenUsage) {
        self.input_tokens += rhs.input_tokens;
        self.output_tokens += rhs.output_tokens;
        self.total_tokens += rhs.total_tokens;
        self.reasoning_tokens += rhs.reasoning_tokens;
        self.cached_tokens += rhs.cached_tokens;
    }
}

/// 工具调用描述（对齐 OpenAI tool_calls 数组元素）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 工具调用唯一 id（tool 消息回填 tool_call_id 用）
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// 对话层结果（单模型 ReAct 一轮的输出）
#[derive(Debug, Clone, Serialize)]
pub struct ChatResult {
    /// 思考过程（数据来自 DeepSeek reasoning_content，字段名 thought 供前端 ThinkingBlock 复用）
    pub thought: String,
    /// 正文（数据来自 content）
    pub message: String,
    /// 本轮模型发起的工具调用（空 = 无需工具，循环结束）
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub usage: TokenUsage,
    /// 生成本结果的模型名（供落盘 / 报表按模型归集）
    #[serde(default)]
    pub model: String,
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
    /// 思考增量（reasoning_content，前端思考块逐字渲染）
    ThoughtDelta { delta: String },
    /// 正文增量（前端逐字渲染）
    MessageDelta { delta: String },
    /// 完成：完整结果（驱动循环、落库）
    Done { result: ChatResult },
}
