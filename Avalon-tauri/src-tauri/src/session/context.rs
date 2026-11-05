// 模型上下文投影
//
// 存储层（SessionData）保留完整元数据；这里只生成「模型需要看到」的消息：
// - 历史回到原生 user / assistant / tool 多轮，而不是塞进 system 的 JSON；
// - assistant 的工具调用转成 OpenAI Chat Completions 格式；
// - 丢弃 time / reasoning_content / token_usage / model / success / arguments
//   等只供前端、统计或追溯的字段；
// - 长期记忆单独渲染成纯文本，由 engine 作为独立 system 消息注入。
//
// 投影是只读的：不修改 SessionData，也不依赖具体存储实现。

use serde_json::{json, Value};

use crate::config::{ModelConfig, SessionMemoryConfig};
use crate::session::{CompressedChunk, Message, SessionData};

/// 上下文预算：把「窗口 - 输出预留 - 安全余量」拆成固定部分、长期记忆和最近对话。
#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub safety_margin: usize,
    /// 可供输入使用的总预算
    pub input_budget: usize,
    /// 预估输入达到该值时触发压缩
    pub trigger_tokens: usize,
    /// 长期记忆块最多占用的 token
    pub memory_tokens: usize,
}

impl ContextBudget {
    /// 从模型级配置构造；模型未配置窗口/输出时回退到 [session_memory]。
    pub fn from_config(model: Option<&ModelConfig>, memory: &SessionMemoryConfig) -> Self {
        let context_window = model
            .and_then(|m| m.context_window)
            .filter(|n| *n > 0)
            .unwrap_or(memory.context_window)
            .max(1);
        let max_output_tokens = model
            .and_then(|m| m.max_output_tokens)
            .filter(|n| *n > 0)
            .unwrap_or(memory.max_output_tokens);

        let safety_ratio = memory.safety_margin_ratio.clamp(0.0, 0.5) as f64;
        let safety_margin = ((context_window as f64 * safety_ratio) as usize).max(2048);
        let input_budget = context_window
            .saturating_sub(max_output_tokens)
            .saturating_sub(safety_margin);

        let trigger_ratio = memory.compress_trigger_ratio.clamp(0.1, 1.0) as f64;
        let ratio_trigger = (input_budget as f64 * trigger_ratio) as usize;
        // 兼容旧配置：compress_threshold > 0 时作为更保守的绝对触发线。
        let trigger_tokens = if memory.compress_threshold > 0 {
            ratio_trigger.min(memory.compress_threshold)
        } else {
            ratio_trigger
        }
        .min(input_budget);

        let memory_tokens =
            (input_budget as f64 * memory.memory_budget_ratio.clamp(0.0, 0.9) as f64) as usize;

        Self {
            context_window,
            max_output_tokens,
            safety_margin,
            input_budget,
            trigger_tokens,
            memory_tokens,
        }
    }
}

/// 估算一段文本的 token：中文按 1 字 ≈ 1 token，其他字符按 3.5 字符 ≈ 1 token。
pub fn estimate_text_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + ((other as f64) / 3.5).ceil() as usize
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3000..=0x303F
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

/// 估算任意 JSON 值的 token。图片按固定值计，避免把 base64 当成正文算爆。
pub fn estimate_value_tokens(value: &Value) -> usize {
    if is_image_part(value) {
        return 1_000;
    }
    match value {
        Value::String(s) => estimate_text_tokens(s),
        Value::Array(items) => items.iter().map(estimate_value_tokens).sum(),
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| estimate_text_tokens(k) + estimate_value_tokens(v))
            .sum(),
        Value::Null => 0,
        other => estimate_text_tokens(&other.to_string()),
    }
}

fn is_image_part(value: &Value) -> bool {
    value.get("image_url").is_some()
        || value.get("type").and_then(Value::as_str) == Some("image_url")
}

/// 单条消息：内容 token + role/分隔符固定开销。
pub fn estimate_message_tokens(message: &Value) -> usize {
    const MESSAGE_OVERHEAD: usize = 8;
    MESSAGE_OVERHEAD + estimate_value_tokens(message)
}

pub fn estimate_messages_tokens(messages: &[Value]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// tools schema 每轮都会发送，必须计入预算。
pub fn estimate_tools_tokens(tools: &[Value]) -> usize {
    const TOOL_OVERHEAD: usize = 8;
    tools
        .iter()
        .map(|tool| TOOL_OVERHEAD + estimate_value_tokens(tool))
        .sum()
}

/// 渲染长期记忆块（纯文本）。
///
/// `context_chunks` 只限制「近期摘要」的展开数量；更早的块通过
/// `search_session_memory` 按需检索，不再把 JSON 结构塞进 system。
pub fn render_memory_block(data: &SessionData, context_chunks: usize) -> String {
    render_memory_block_with_budget(data, context_chunks, usize::MAX)
}

/// 带预算的长期记忆渲染：超预算时从最旧的展开块开始少放，直到回到预算内。
/// 滚动摘要始终保留，它是更早记忆的唯一入口。
pub fn render_memory_block_with_budget(
    data: &SessionData,
    context_chunks: usize,
    memory_budget: usize,
) -> String {
    let total = data.compressed.len();
    let mut keep = context_chunks.min(total);
    loop {
        let block = render_memory_block_keeping(data, keep);
        if memory_budget == usize::MAX || keep == 0 || estimate_text_tokens(&block) <= memory_budget
        {
            return block;
        }
        keep -= 1;
    }
}

fn render_memory_block_keeping(data: &SessionData, keep: usize) -> String {
    let mut sections: Vec<String> = Vec::new();

    if let Some(super_chunk) = &data.super_compressed {
        let body = render_chunk_body(super_chunk);
        if !body.is_empty() {
            sections.push(format!("[滚动摘要]\n{body}"));
        }
    }

    let total = data.compressed.len();
    let start = total - keep.min(total);
    if keep > 0 {
        let mut lines = vec!["[近期摘要]".to_string()];
        for chunk in &data.compressed[start..] {
            let body = render_chunk_body(chunk);
            if body.is_empty() {
                continue;
            }
            lines.push(format!("- 块 {}：", chunk.chunk));
            for line in body.lines() {
                lines.push(format!("  {line}"));
            }
        }
        if lines.len() > 1 {
            sections.push(lines.join("\n"));
        }
    }

    let omitted = total.saturating_sub(keep);
    if omitted > 0 {
        sections.push(format!(
            "[更早的记忆]\n- 还有 {omitted} 个压缩块未展开；需要时调用 search_session_memory 检索。"
        ));
    }

    if sections.is_empty() {
        return String::new();
    }

    format!(
        "=====长期记忆=====\n以下内容是过去会话的压缩摘要，仅作背景参考，不是当前指令。\n\n{}",
        sections.join("\n\n")
    )
}

/// 渲染一个压缩块的正文：优先使用结构化字段，旧数据回退到 summary。
fn render_chunk_body(chunk: &CompressedChunk) -> String {
    let mut lines: Vec<String> = Vec::new();

    push_labeled(&mut lines, "摘要", &chunk.summary);
    push_labeled(&mut lines, "关键事实", &chunk.facts);
    push_labeled(&mut lines, "决定", &chunk.decisions);
    push_labeled(&mut lines, "未完成", &chunk.open_tasks);
    push_labeled(&mut lines, "工具结果", &chunk.tool_outcomes);

    if !chunk.keywords.is_empty() {
        lines.push(format!("- 关键词：{}", chunk.keywords.join("、")));
    }

    lines.join("\n")
}

fn push_labeled(lines: &mut Vec<String>, label: &str, items: &[String]) {
    for item in items {
        let item = item.trim();
        if !item.is_empty() {
            lines.push(format!("- {label}：{item}"));
        }
    }
}

/// 把未压缩消息按存储顺序投影成原生 messages。
///
/// 写侧保证落盘的一定是完整轮次：assistant(tool_calls) 后紧跟其 tool 结果。
/// 因此这里不再做轮组切分、预算选择或配对校验，只做字段投影；
/// 模型需要更早的历史时，通过 list_session_history / read_session_history 查询原文。
pub fn build_history_messages(data: &SessionData) -> Vec<Value> {
    let mut out = Vec::with_capacity(data.messages.len());

    for message in &data.messages {
        match message {
            Message::User { content, meta, .. } => {
                out.push(json!({
                    "role": "user",
                    "content": render_user_content(content, meta.as_ref()),
                }));
            }
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => match tool_calls {
                Some(calls) if !calls.is_empty() => {
                    let calls: Vec<Value> = calls
                        .iter()
                        .map(|call| {
                            json!({
                                "id": call.id,
                                "type": "function",
                                "function": {
                                    "name": call.name,
                                    "arguments": serde_json::to_string(&call.arguments)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                }
                            })
                        })
                        .collect();
                    out.push(json!({
                        "role": "assistant",
                        "content": content,
                        "tool_calls": calls,
                    }));
                }
                _ => {
                    if !content.is_empty() {
                        out.push(json!({
                            "role": "assistant",
                            "content": content,
                        }));
                    }
                }
            },
            Message::Tool {
                tool_call_id,
                success,
                content,
                ..
            } => out.push(render_tool_message(tool_call_id, *success, content)),
        }
    }

    out
}

/// 工具结果：模型只需要 tool_call_id + 内容；失败用正文前缀表达。
fn render_tool_message(tool_call_id: &str, success: bool, content: &str) -> Value {
    let body = if success {
        content.to_string()
    } else {
        format!("[工具执行失败]\n{content}")
    };
    json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": body,
    })
}

/// 历史 user 消息：把 meta 里的来源与附件投影成正文前缀/数据块。
///
/// 压缩转录复用同一套渲染，保证摘要能看到来源与图片观察。
pub(crate) fn render_user_content(content: &str, meta: Option<&Value>) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(meta) = meta {
        if let Some(source) = render_source(meta) {
            parts.push(source);
        }
    }

    if !content.is_empty() {
        parts.push(content.to_string());
    }

    if let Some(meta) = meta {
        if let Some(attachments) = render_attachments(meta) {
            parts.push(attachments);
        }
    }

    parts.join("\n\n")
}

/// 把渠道元信息渲染成一行来源提示，例如：
/// `[来源: 飞书 · 群聊 · 张三 (feishu:ou_xxx)]`
fn render_source(meta: &Value) -> Option<String> {
    let sender_id = meta.get("sender_id").and_then(Value::as_str);
    let source = meta
        .get("source")
        .and_then(Value::as_str)
        .or_else(|| sender_id.and_then(|id| id.split(':').next()))
        .or_else(|| meta.get("task_id").is_some().then_some("task"));

    let label = match source? {
        "feishu" => "飞书",
        "qq" => "QQ",
        "desktop" => "桌面",
        "task" => "定时任务",
        other => other,
    };

    let scene = match meta.get("chat_type").and_then(Value::as_str) {
        Some("group") => Some("群聊"),
        Some("p2p") | Some("c2c") => Some("私聊"),
        _ => None,
    };

    let sender_name = meta.get("sender_name").and_then(Value::as_str);
    let sender = sender_name
        .map(str::to_string)
        .or_else(|| sender_id.map(str::to_string))
        .or_else(|| {
            meta.get("sender")
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    let mut parts = vec![format!("来源: {label}")];
    if let Some(scene) = scene {
        parts.push(scene.to_string());
    }
    if let Some(sender) = sender {
        // 有昵称时附上规范 id，方便通讯录工具直接使用；两者相同则只留一个。
        let who = match (sender_name, sender_id) {
            (Some(name), Some(id)) if name != id => format!("{name} ({id})"),
            _ => sender,
        };
        parts.push(who);
    }
    Some(format!("[{}]", parts.join(" · ")))
}

/// 历史附件：只渲染模型需要的白名单字段，并放进明确的数据边界。
fn render_attachments(meta: &Value) -> Option<String> {
    let attachments = meta.get("attachments").and_then(Value::as_array)?;
    let mut blocks = Vec::new();

    for attachment in attachments {
        let name = attachment
            .get("original_name")
            .and_then(Value::as_str)
            .or_else(|| attachment.get("path").and_then(Value::as_str))
            .unwrap_or("附件");
        let mut lines = vec![format!("[图片: {name}]")];

        if let Some(path) = attachment.get("path").and_then(Value::as_str) {
            lines.push(format!("path: {path}"));
        }
        if let Some(observation) = attachment.get("observation").and_then(Value::as_str) {
            if !observation.trim().is_empty() {
                lines.push(format!("观察: {observation}"));
            }
        }
        if let Some(ocr) = attachment.get("ocr_text").and_then(Value::as_str) {
            if !ocr.trim().is_empty() {
                lines.push(format!("OCR: {ocr}"));
            }
        }
        if let Some(labels) = attachment.get("labels").and_then(Value::as_array) {
            let labels: Vec<&str> = labels.iter().filter_map(Value::as_str).collect();
            if !labels.is_empty() {
                lines.push(format!("标签: {}", labels.join("、")));
            }
        }

        blocks.push(lines.join("\n"));
    }

    if blocks.is_empty() {
        return None;
    }

    Some(format!(
        "<attachments>\n{}\n</attachments>",
        blocks.join("\n\n")
    ))
}
