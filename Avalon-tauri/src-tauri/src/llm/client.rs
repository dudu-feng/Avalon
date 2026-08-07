// LLM 客户端 —— 封装 OpenAI 兼容 API 的 HTTP 调用
//
// 提供三种调用模式：
//   1. chat()     — 对话层调用，标准模型，返回 JSON 格式响应
//   2. action()   — 动作层调用，强制 JSON 输出，执行分步骤任务
//   3. compress() — 会话压缩调用，强制 JSON 输出，返回摘要和关键词

use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

use super::prompts;
use super::types::{ChatRequest, ChatResponse, LlmMessage, LlmResponse, ResponseFormat};
use crate::config::AppConfig;

// ============================================================
//  LLM 客户端
// ============================================================

#[derive(Clone)]
pub struct LlmClient {
    /// HTTP 客户端（复用连接池）
    http: Client,
    /// API Key
    api_key: String,
    /// 模型名称
    model: String,
    /// API 基础 URL（如 "https://xxx/v1"）
    base_url: String,
}

impl LlmClient {
    /// 从配置创建 LLM 客户端
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("创建 HTTP 客户端失败")?;

        Ok(LlmClient {
            http,
            api_key: config.llm.api_key.clone(),
            model: config.llm.model.clone(),
            base_url: config.llm.base_url.clone(),
        })
    }

    // ============================================================
    //  公开接口
    // ============================================================

    /// 对话层 LLM 调用
    ///
    /// 接收用户输入和系统提示，返回模型响应。
    /// 不强制 JSON 输出（模型仍被引导返回 JSON，但允许降级为纯文本）。
    pub async fn chat(
        &self,
        system_prompt: &str,
        user_input: &str,
        chat_history: &str,
    ) -> Result<LlmResponse> {
        let messages = vec![
            LlmMessage::System(system_prompt.to_string()),
            LlmMessage::User(user_input.to_string()),
            LlmMessage::Assistant(chat_history.to_string()),
        ];

        self.invoke(messages, None).await
    }

    /// 动作层 LLM 调用
    ///
    /// 执行分步骤任务，强制 JSON 输出。
    /// 返回 JSON 包含 next/tool_call/sub_analysis 等字段。
    pub async fn action(
        &self,
        user_input: &str,
        action_target: &str,
        action_history: &str,
    ) -> Result<LlmResponse> {
        let system_prompt =
            prompts::build_action_prompt(action_target, action_history);

        let messages = vec![
            LlmMessage::System(system_prompt),
            LlmMessage::User(user_input.to_string()),
        ];

        self.invoke(messages, Some(ResponseFormat::json_object())).await
    }

    /// 会话压缩 LLM 调用
    ///
    /// 将历史会话压缩为摘要和关键词，强制 JSON 输出。
    /// 返回 JSON 包含 summary（数组）和 keywords（数组）。
    pub async fn compress(&self, session_data: &str) -> Result<LlmResponse> {
        let messages = vec![
            LlmMessage::System(prompts::COMPRESS_SYSTEM_PROMPT.to_string()),
            LlmMessage::User(prompts::build_compress_user_prompt(session_data)),
        ];

        self.invoke(messages, Some(ResponseFormat::json_object())).await
    }

    // ============================================================
    //  内部方法
    // ============================================================

    /// 发送 HTTP 请求到 OpenAI 兼容 API
    async fn invoke(
        &self,
        messages: Vec<LlmMessage>,
        response_format: Option<ResponseFormat>,
    ) -> Result<LlmResponse> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            response_format,
        };

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await
            .context(format!("LLM API 请求失败: {}", url))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API 返回错误状态码 {}: {}", status, body);
        }

        let chat_response: ChatResponse = resp
            .json()
            .await
            .context("解析 LLM API 响应失败")?;

        Ok(LlmResponse::from(chat_response))
    }
}
