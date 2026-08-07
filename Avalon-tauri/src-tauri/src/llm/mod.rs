// LLM 交互模块
//
// 封装与 OpenAI 兼容 API 的交互，提供三种调用模式：
//   - chat:    对话层调用，返回 JSON（thought/message/next/action_target）
//   - action:  动作层调用，返回 JSON（analysis/next/tool_call/sub_analysis）
//   - compress: 会话压缩调用，返回 JSON（summary/keywords）

pub mod client;
pub mod prompts;
pub mod types;

pub use client::LlmClient;
pub use types::LlmResponse;
