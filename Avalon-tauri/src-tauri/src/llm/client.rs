// 异步 LLM 客户端
//
// 只依赖 config::LlmConfig（依赖反转），不 import 工具/会话/提示词模块。
// 提供三个调用入口：chat_stream（流式）/ action（非流式 JSON）/ compress（非流式 JSON）。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::config::LlmConfig;

use super::parser::parse_llm_json;
use super::stream::StreamParser;
use super::types::*;

/// 共享 HTTP 客户端状态（复用连接池，避免每次请求重建）
pub struct LlmState {
    http: reqwest::Client,
}

impl LlmState {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    /// 从配置快照构建客户端（配置变更后重建即生效，替代 Python 单例 refresh_models）
    pub fn client(&self, cfg: LlmConfig) -> LlmClient {
        LlmClient::new(self.http.clone(), cfg)
    }
}

pub struct LlmClient {
    http: reqwest::Client,
    cfg: LlmConfig,
}

impl LlmClient {
    pub fn new(http: reqwest::Client, cfg: LlmConfig) -> Self {
        Self { http, cfg }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'))
    }

    /// 对话层：流式调用，逐字推正文/思考，流结束产出 ChatResult（含用量）
    pub async fn chat_stream(
        &self,
        system_prompt: &str,
        user_input: &str,
        chat_history: &str,
        mut on_event: impl FnMut(StreamEvent),
    ) -> Result<ChatResult> {
        let mut messages = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": user_input}),
        ];
        if !chat_history.trim().is_empty() {
            messages.push(json!({"role": "assistant", "content": chat_history}));
        }

        let body = json!({
            "model": self.cfg.model,
            "messages": messages,
            "temperature": self.cfg.chat_temperature,
            "stream": true,
            "stream_options": {"include_usage": true},
        });

        let resp = self.post(&body).await?;

        let mut parser = StreamParser::new();
        let mut usage = TokenUsage::default();
        let mut sse_buf = String::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("读取流失败")?;
            sse_buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some((event, rest)) = take_sse_event(&sse_buf) {
                sse_buf = rest;
                if let Some(delta) = sse_delta(&event) {
                    parser.push(&delta, &mut on_event);
                }
                if let Some(u) = sse_usage(&event) {
                    usage = u;
                }
            }
        }

        // 流结束后处理无尾随空行的残余事件（通常是最后一个 usage / [DONE]）
        if !sse_buf.trim().is_empty() {
            if let Some(delta) = sse_delta(&sse_buf) {
                parser.push(&delta, &mut on_event);
            }
            if let Some(u) = sse_usage(&sse_buf) {
                usage = u;
            }
        }

        let mut result = parser.finish();
        result.usage = usage;
        on_event(StreamEvent::Done {
            result: result.clone(),
        });
        Ok(result)
    }

    /// 动作层：非流式，低温度 JSON（含 response_format 降级）。
    /// system_prompt 由 engine 用 prompt::build_action_prompt 组装后传入。
    pub async fn action(&self, system_prompt: &str) -> Result<ActionResult> {
        let (content, usage) = self.invoke_json(system_prompt, None).await?;
        let mut result: ActionResult = parse_llm_json(&content)?;
        result.usage = usage;
        Ok(result)
    }

    /// 压缩层：非流式，低温度 JSON。
    /// (system_prompt, user_prompt) 由 engine 用 prompt::build_compress_prompt 组装后传入。
    pub async fn compress(&self, system_prompt: &str, user_prompt: &str) -> Result<CompressResult> {
        let (content, usage) = self.invoke_json(system_prompt, Some(user_prompt)).await?;
        let mut result: CompressResult = parse_llm_json(&content)?;
        result.usage = usage;
        Ok(result)
    }

    /// 发送请求并校验状态码，返回响应体
    async fn post(&self, body: &Value) -> Result<reqwest::Response> {
        let resp = self
            .http
            .post(self.endpoint())
            .bearer_auth(&self.cfg.api_key)
            .timeout(Duration::from_secs(self.cfg.timeout_secs))
            .json(body)
            .send()
            .await
            .context("请求 LLM 失败")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("LLM 请求失败 {}: {}", status.as_u16(), text));
        }
        Ok(resp)
    }

    /// 非流式 JSON 调用：优先 response_format=json_object，空 content / 异常时降级重试
    async fn invoke_json(
        &self,
        system_prompt: &str,
        user_prompt: Option<&str>,
    ) -> Result<(String, TokenUsage)> {
        let mut messages = vec![json!({"role": "system", "content": system_prompt})];
        if let Some(u) = user_prompt {
            messages.push(json!({"role": "user", "content": u}));
        }

        match self.invoke(&messages, true).await {
            Ok((content, usage)) if !content.trim().is_empty() => Ok((content, usage)),
            Ok(_) => self.invoke(&messages, false).await,
            Err(_) => self.invoke(&messages, false).await,
        }
    }

    /// 单次非流式调用，返回 (content, usage)
    async fn invoke(
        &self,
        messages: &[Value],
        json_mode: bool,
    ) -> Result<(String, TokenUsage)> {
        let mut body = json!({
            "model": self.cfg.model,
            "messages": messages,
            "temperature": self.cfg.json_temperature,
            "stream": false,
        });
        if json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }

        let resp = self.post(&body).await?;
        let v: Value = resp.json().await.context("解析响应 JSON 失败")?;

        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let usage = parse_usage(&v);
        Ok((content, usage))
    }
}

// ============ SSE 解析辅助 ============

/// 从缓冲区提取一个完整 SSE 事件（以空行分隔），返回 (事件, 剩余)。
fn take_sse_event(buf: &str) -> Option<(String, String)> {
    if let Some(idx) = buf.find("\n\n") {
        let event = buf[..idx].to_string();
        let rest = buf[idx + 2..].to_string();
        return Some((event, rest));
    }
    if let Some(idx) = buf.find("\r\n\r\n") {
        let event = buf[..idx].to_string();
        let rest = buf[idx + 4..].to_string();
        return Some((event, rest));
    }
    None
}

/// 从 SSE 事件中提取 delta.content（无则返回 None）。
fn sse_delta(event: &str) -> Option<String> {
    for line in event.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                return None;
            }
            let v: Value = serde_json::from_str(data).ok()?;
            return v["choices"][0]["delta"]["content"].as_str().map(|s| s.to_string());
        }
    }
    None
}

/// 从 SSE 事件中提取 usage（无则返回 None）。
fn sse_usage(event: &str) -> Option<TokenUsage> {
    for line in event.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                continue;
            }
            let v: Value = serde_json::from_str(data).ok()?;
            if let Some(u) = v.get("usage") {
                return Some(parse_usage(u));
            }
        }
    }
    None
}

/// 从 usage 对象提取 token 用量（OpenAI 字段名）。
fn parse_usage(u: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
    }
}
