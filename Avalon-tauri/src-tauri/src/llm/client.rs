// 异步 LLM 客户端
//
// 只依赖 config::{ModelConfig, LlmParams}（依赖反转），不 import 工具/会话/提示词模块。
// 提供两个调用入口：chat_stream（带 tools 的流式，单模型 ReAct）/ compress（非流式 JSON）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::config::{LlmParams, ModelConfig};

use super::parser::parse_llm_json;
use super::types::*;

/// 共享 HTTP 客户端状态（复用连接池，避免每次请求重建）。
/// Clone 为浅拷贝（reqwest::Client 内部 Arc），供 session 等模块克隆共享。
/// 建立连接（TCP + TLS）的超时：只罩握手阶段，不随流式时长累积。
const CONNECT_TIMEOUT_SECS: u64 = 30;

#[derive(Clone)]
pub struct LlmState {
    http: reqwest::Client,
}

impl LlmState {
    /// 构造共享 HTTP 客户端。
    /// 连接级超时在这里定死：connect_timeout 只罩握手；read_timeout 按 timeout_secs 做
    /// 逐块空闲超时（每收到一块就重置）。流式请求绝不能套「总超时」—— 思考/长回答的流
    /// 会超过 timeout_secs，reqwest 会在流中间掐断连接，表现为「读取流失败」。
    pub fn new(timeout_secs: u64) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .read_timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("构建 HTTP 客户端失败");
        Self { http }
    }

    /// 从配置快照构建客户端（配置变更后重建即生效，替代 Python 单例 refresh_models）
    pub fn client(&self, model: ModelConfig, params: LlmParams) -> LlmClient {
        LlmClient::new(self.http.clone(), model, params)
    }
}

pub struct LlmClient {
    http: reqwest::Client,
    model: ModelConfig,
    params: LlmParams,
}

impl LlmClient {
    pub fn new(http: reqwest::Client, model: ModelConfig, params: LlmParams) -> Self {
        Self { http, model, params }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.model.url.trim_end_matches('/'))
    }

    /// 对话层：带 tools 的流式调用。
    /// `messages` 由调用方构造（含 system/user/历史 assistant(tool_calls)/tool 消息），
    /// 流式推正文（content）/思考（reasoning_content），流结束产出 ChatResult（含 tool_calls）。
    pub async fn chat_stream(
        &self,
        messages: &[Value],
        tools: &[Value],
        cancel: &AtomicBool,
        mut on_event: impl FnMut(StreamEvent),
    ) -> Result<ChatResult> {
        let mut body = json!({
            "model": self.model.modelname,
            "messages": messages,
            "temperature": self.params.chat_temperature,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !tools.is_empty() {
            body["tools"] = json!(tools);
            body["tool_choice"] = json!("auto");
        }

        let resp = self.post(&body, true).await?;

        let mut usage = TokenUsage::default();
        let mut thought = String::new();
        let mut message = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut sse_buf = String::new();
        let mut stream = resp.bytes_stream();
        let mut cancelled = false;

        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let chunk = chunk.context("读取流失败")?;
            sse_buf.push_str(&String::from_utf8_lossy(&chunk));

            while let Some((event, rest)) = take_sse_event(&sse_buf) {
                sse_buf = rest;
                if let Some(delta) = sse_delta(&event) {
                    apply_delta(&delta, &mut thought, &mut message, &mut tool_calls, &mut on_event);
                }
                if let Some(u) = sse_usage(&event) {
                    usage = u;
                }
            }
        }

        // 流结束后处理无尾随空行的残余事件（通常是最后一个 usage / [DONE]）；
        // 取消时跳过，避免把取消前的缓冲继续推给前端。
        if !cancelled && !sse_buf.trim().is_empty() {
            if let Some(delta) = sse_delta(&sse_buf) {
                apply_delta(&delta, &mut thought, &mut message, &mut tool_calls, &mut on_event);
            }
            if let Some(u) = sse_usage(&sse_buf) {
                usage = u;
            }
        }

        // 工具调用 arguments 在流式阶段是 JSON 字符串片段，此处统一解析为 Value
        for tc in &mut tool_calls {
            if let Some(s) = tc.arguments.as_str() {
                tc.arguments = serde_json::from_str(s).unwrap_or(Value::Null);
            }
        }

        let result = ChatResult {
            thought,
            message,
            tool_calls,
            usage,
            model: self.model.modelname.clone(),
        };
        on_event(StreamEvent::Done {
            result: result.clone(),
        });
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

    /// 发送请求并校验状态码，返回响应体。
    /// `streaming` 决定是否套「总超时」：非流式（compress / JSON 动作）要总超时防挂起；
    /// 流式不能套 —— 长思考/长回答会超过 timeout_secs，总超时会在流中间掐断连接。
    /// 流式的空闲检测由客户端级 read_timeout 承担（见 LlmState::new）。
    async fn post(&self, body: &Value, streaming: bool) -> Result<reqwest::Response> {
        let mut req = self
            .http
            .post(self.endpoint())
            .bearer_auth(&self.model.key)
            .json(body);
        if !streaming {
            req = req.timeout(Duration::from_secs(self.params.timeout_secs));
        }

        let resp = req.send().await.context("请求 LLM 失败")?;

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
            "model": self.model.modelname,
            "messages": messages,
            "temperature": self.params.json_temperature,
            "stream": false,
        });
        if json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }

        let resp = self.post(&body, false).await?;
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
pub(crate) fn take_sse_event(buf: &str) -> Option<(String, String)> {
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

/// 从 SSE 事件中提取 delta 对象（choices[0].delta，无则返回 None）。
pub(crate) fn sse_delta(event: &str) -> Option<Value> {
    for line in event.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                return None;
            }
            let v: Value = serde_json::from_str(data).ok()?;
            return Some(v["choices"][0]["delta"].clone());
        }
    }
    None
}

/// 累加一个 delta 增量：正文 / 思考 / 工具调用，并推流式事件。
pub(crate) fn apply_delta(
    delta: &Value,
    thought: &mut String,
    message: &mut String,
    tool_calls: &mut Vec<ToolCall>,
    on_event: &mut dyn FnMut(StreamEvent),
) {
    // 正文增量（content）
    if let Some(c) = delta.get("content").and_then(Value::as_str) {
        if !c.is_empty() {
            message.push_str(c);
            on_event(StreamEvent::MessageDelta { delta: c.to_string() });
        }
    }

    // 思考增量（DeepSeek reasoning_content，换其他模型可能缺省）
    if let Some(r) = delta.get("reasoning_content").and_then(Value::as_str) {
        if !r.is_empty() {
            thought.push_str(r);
            on_event(StreamEvent::ThoughtDelta { delta: r.to_string() });
        }
    }

    // 工具调用增量（tool_calls 数组，按 index 累加；首片带 id/name，arguments 跨 chunk 拼接）
    if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
        for tc in tcs {
            let index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            while tool_calls.len() <= index {
                tool_calls.push(ToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: Value::Null,
                });
            }
            let slot = &mut tool_calls[index];
            if let Some(id) = tc.get("id").and_then(Value::as_str) {
                slot.id = id.to_string();
            }
            if let Some(f) = tc.get("function") {
                if let Some(name) = f.get("name").and_then(Value::as_str) {
                    slot.name = name.to_string();
                }
                if let Some(args) = f.get("arguments").and_then(Value::as_str) {
                    // arguments 是 JSON 字符串片段，跨 chunk 拼接
                    match slot.arguments.as_str() {
                        Some(existing) => {
                            slot.arguments = Value::String(format!("{existing}{args}"));
                        }
                        None => {
                            slot.arguments = Value::String(args.to_string());
                        }
                    }
                }
            }
        }
    }
}

/// 从 SSE 事件中提取 usage（无则返回 None）。
pub(crate) fn sse_usage(event: &str) -> Option<TokenUsage> {
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
pub(crate) fn parse_usage(u: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        reasoning_tokens: u["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap_or(0) as u32,
        cached_tokens: u["prompt_tokens_details"]["cached_tokens"]
            .as_u64()
            .unwrap_or(0) as u32,
    }
}
