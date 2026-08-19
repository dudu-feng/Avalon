// LLM 模型调用层模块
//
// 职责：封装 OpenAI 兼容 API 调用，流式输出正文/思考 + 结构化返回控制字段。
// 仅依赖 config 模块，不耦合工具/会话/提示词组装（依赖反转）。

pub mod client;
pub mod parser;
pub mod types;

#[allow(unused_imports)] // LlmClient 供未来 engine 层显式引用
pub use client::{LlmClient, LlmState};
pub use types::*;
