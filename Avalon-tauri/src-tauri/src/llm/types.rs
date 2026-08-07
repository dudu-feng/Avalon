// LLM 类型定义
//
// 消息类型、API 请求/响应结构、Token 使用统计

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ============================================================
//  消息类型
// ============================================================

/// 聊天消息枚举，对应 OpenAI API 的 messages 数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", content = "content")]
pub enum LlmMessage {
    /// 系统提示
    #[serde(rename = "system")]
    System(String),

    /// 用户输入
    #[serde(rename = "user")]
    User(String),

    /// AI 回复
    #[serde(rename = "assistant")]
    Assistant(String),
}

impl LlmMessage {
    /// 获取消息角色字符串
    pub fn role(&self) -> &'static str {
        match self {
            LlmMessage::System(_) => "system",
            LlmMessage::User(_) => "user",
            LlmMessage::Assistant(_) => "assistant",
        }
    }

    /// 获取消息内容
    pub fn content(&self) -> &str {
        match self {
            LlmMessage::System(c) | LlmMessage::User(c) | LlmMessage::Assistant(c) => c,
        }
    }
}

// ============================================================
//  API 请求结构
// ============================================================

/// OpenAI Chat Completions 请求体
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,

    /// 是否强制 JSON 输出（通过 response_format 设置）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
}

impl ResponseFormat {
    /// 创建 JSON 强制输出格式
    pub fn json_object() -> Self {
        ResponseFormat {
            format_type: "json_object".to_string(),
        }
    }
}

// ============================================================
//  API 响应结构
// ============================================================

/// OpenAI Chat Completions 响应体
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ResponseMessage,
    pub finish_reason: String,
}

#[derive(Debug, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

// ============================================================
//  对外返回的统一响应
// ============================================================

/// LLM 调用结果，封装了文本内容和 token 使用统计
#[derive(Debug, Clone, Serialize)]
pub struct LlmResponse {
    /// LLM 返回的原始文本内容
    pub content: String,
    /// Token 使用统计
    pub token_usage: TokenUsage,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

impl From<Usage> for TokenUsage {
    fn from(usage: Usage) -> Self {
        TokenUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

impl From<ChatResponse> for LlmResponse {
    fn from(resp: ChatResponse) -> Self {
        let content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        LlmResponse {
            content,
            token_usage: TokenUsage::from(resp.usage),
        }
    }
}
