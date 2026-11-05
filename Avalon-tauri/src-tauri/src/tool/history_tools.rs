// 会话历史原文工具
//
// search_session_memory 查的是向量库里的压缩摘要；这两个工具直接读会话存储，
// 返回原始消息。读写都走 SessionStore，不经过文件沙箱，所以不会因为
// data/memory/session 不在工作区而失败。
//
// 仅主人可用：统一记忆下所有渠道共用一份会话，原文里可能包含私聊内容。

use serde_json::Value;

use crate::session::artifact::{DEFAULT_READ_LIMIT, MAX_READ_LIMIT};
use crate::session::{Message, SessionStatus, SessionStore};

use super::ToolContext;

/// 单条消息最多展示字符数
const MAX_MESSAGE_CHARS: usize = 2000;
/// 单次返回的总字符上限
const MAX_TOTAL_CHARS: usize = 20000;
/// 默认读取条数
const DEFAULT_LIMIT: usize = 20;
/// 单次读取条数上限
const MAX_LIMIT: usize = 100;

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn bool_arg(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn status_label(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "进行中",
        SessionStatus::Archived => "已归档",
        SessionStatus::Inactive => "未开始",
    }
}

/// 列表：跨渠道列出所有已保存会话
pub fn list_session_history(args: &Value, session: &dyn SessionStore, ctx: &ToolContext) -> String {
    if !ctx.is_owner {
        return "权限不足: 会话历史只对主人开放".to_string();
    }

    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200) as usize;

    let sessions = match session.list_all_sessions() {
        Ok(list) => list,
        Err(e) => return format!("会话历史读取失败: {e:#}"),
    };
    if sessions.is_empty() {
        return "没有已保存的会话".to_string();
    }

    let mut lines = vec![format!("会话列表（共 {} 个）：", sessions.len())];
    for meta in sessions.iter().take(limit) {
        let title = if meta.title.trim().is_empty() {
            "未命名"
        } else {
            meta.title.trim()
        };
        lines.push(format!(
            "- {} [{}] {}",
            meta.id,
            status_label(meta.status),
            title
        ));
    }
    if sessions.len() > limit {
        lines.push(format!("……仅显示前 {limit} 个，共 {} 个", sessions.len()));
    }
    lines.push(
        "读取原文请调用 read_session_history，session_id 传上面的 id；不传则读当前会话。"
            .to_string(),
    );
    lines.join("\n")
}

/// 读取会话原文。默认从最新消息往前分页，返回时按时间正序展示
pub fn read_session_history(args: &Value, session: &dyn SessionStore, ctx: &ToolContext) -> String {
    if !ctx.is_owner {
        return "权限不足: 会话历史只对主人开放".to_string();
    }

    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIMIT as u64)
        .clamp(1, MAX_LIMIT as u64) as usize;
    let include_tools = bool_arg(args, "include_tools");
    let include_reasoning = bool_arg(args, "include_reasoning");

    // session_id 缺省时读当前会话；显式 id 时从会话列表补标题与状态
    let (id, title, status) = match str_arg(args, "session_id").or_else(|| str_arg(args, "id")) {
        Some(id) => {
            let meta = session
                .list_all_sessions()
                .ok()
                .and_then(|list| list.into_iter().find(|m| m.id == id));
            match meta {
                Some(meta) => (meta.id, meta.title, meta.status),
                None => return format!("会话历史读取失败: 会话 '{id}' 不存在"),
            }
        }
        None => match session.get_current_session(&ctx.session_key) {
            Ok(data) if !data.id.is_empty() => (data.id, data.title, data.status),
            Ok(_) => return "当前没有进行中的会话".to_string(),
            Err(e) => return format!("会话历史读取失败: {e:#}"),
        },
    };

    let messages = match session.read_session_raw_messages(&id) {
        Ok(messages) => messages,
        Err(e) => return format!("会话历史读取失败: {e:#}"),
    };

    let total = messages.len();
    let mut out = vec![
        format!(
            "会话：{}（{}）",
            if title.trim().is_empty() {
                "未命名"
            } else {
                title.trim()
            },
            id
        ),
        format!("状态：{}；原文共 {total} 条", status_label(status)),
    ];

    if total == 0 {
        out.push("该会话没有消息。".to_string());
        return out.join("\n");
    }
    if offset >= total {
        out.push(format!(
            "offset={offset} 超出范围（共 {total} 条）。offset=0 表示从最新消息开始。"
        ));
        return out.join("\n");
    }

    let end = total - offset;
    let start = end.saturating_sub(limit);
    let mut rendered = Vec::new();
    let mut used_chars = 0usize;
    let mut consumed = 0usize;

    // 从新到旧取，凑够字符上限后再反转成时间正序
    for message in messages[start..end].iter().rev() {
        if !include_tools && matches!(message, Message::Tool { .. }) {
            consumed += 1;
            continue;
        }
        let block = format_message(message, include_reasoning);
        if block.is_empty() {
            consumed += 1;
            continue;
        }
        let block_chars = block.chars().count();
        if !rendered.is_empty() && used_chars + block_chars > MAX_TOTAL_CHARS {
            break;
        }
        used_chars += block_chars;
        rendered.push(block);
        consumed += 1;
    }
    rendered.reverse();

    out.push(format!(
        "分页：offset={offset} limit={limit}，本次返回 {} 条（按时间正序）",
        rendered.len()
    ));
    if rendered.is_empty() {
        out.push("本次范围内只有工具消息；需要查看时传 include_tools=true。".to_string());
    } else {
        out.push(String::new());
        out.extend(rendered);
    }

    let next_offset = offset + consumed;
    if end.saturating_sub(consumed) > 0 {
        out.push(String::new());
        out.push(format!(
            "还有更早的消息，继续读取请传 offset={next_offset}。"
        ));
    }
    if offset > 0 {
        out.push(format!(
            "还有更晚的消息，减小 offset（当前 {offset}）即可查看。"
        ));
    }
    if !include_tools {
        out.push("本次未显示工具执行结果；需要时 include_tools=true。".to_string());
    }

    out.join("\n")
}

/// 读取当前会话中工具结果的完整正文。
///
/// artifact id 来自工具消息末尾的 `[完整结果: artifact:xxx]`。只允许读
/// 当前会话：切换会话后旧 artifact 不再暴露，避免跨会话越权读取。
pub fn read_tool_result(args: &Value, session: &dyn SessionStore, ctx: &ToolContext) -> String {
    let Some(raw_id) = str_arg(args, "artifact_id").or_else(|| str_arg(args, "id")) else {
        return "参数错误: 缺少 artifact_id（来自工具消息里的 [完整结果: artifact:xxx]）"
            .to_string();
    };
    let artifact_id = raw_id.strip_prefix("artifact:").unwrap_or(raw_id).trim();

    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_READ_LIMIT)
        .clamp(1, MAX_READ_LIMIT);

    let read = match session.read_tool_artifact(&ctx.session_key, artifact_id, offset, limit) {
        Ok(read) => read,
        Err(e) => return format!("读取工具结果失败: {e:#}"),
    };

    let mut out = vec![format!(
        "[artifact:{}] 字符 {}-{} / 共 {}",
        read.id, read.offset, read.next_offset, read.total_chars
    )];
    if read.content.is_empty() {
        out.push("（该偏移之后没有内容，offset 从 0 开始）".to_string());
        return out.join("\n");
    }
    out.push(String::new());
    out.push(read.content);
    if read.next_offset < read.total_chars {
        out.push(String::new());
        out.push(format!(
            "还有更多内容，继续读取请传 offset={}。",
            read.next_offset
        ));
    }
    out.join("\n")
}

fn format_message(message: &Message, include_reasoning: bool) -> String {
    match message {
        Message::User {
            time,
            content,
            meta,
        } => {
            let who = sender_label(meta.as_ref());
            format!("[{time}] 用户{who}\n{}", clip(content))
        }
        Message::Assistant {
            time,
            content,
            reasoning_content,
            tool_calls,
            model,
            ..
        } => {
            let mut out = format!(
                "[{time}] 助手{}",
                if model.trim().is_empty() {
                    String::new()
                } else {
                    format!("（{}）", model.trim())
                }
            );
            if !content.trim().is_empty() {
                out.push('\n');
                out.push_str(&clip(content));
            }
            if let Some(calls) = tool_calls.as_ref().filter(|c| !c.is_empty()) {
                let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
                out.push_str(&format!("\n[工具调用] {}", names.join("、")));
            }
            if include_reasoning {
                if let Some(reasoning) = reasoning_content
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    out.push_str(&format!("\n[思考] {}", clip(reasoning)));
                }
            }
            out
        }
        Message::Tool {
            time,
            name,
            success,
            content,
            ..
        } => format!(
            "[{time}] 工具 {name}（{}）\n{}",
            if *success { "成功" } else { "失败" },
            clip(content)
        ),
    }
}

/// 用户消息的来源标签：优先 sender_name，其次规范 id
fn sender_label(meta: Option<&Value>) -> String {
    let Some(meta) = meta else {
        return String::new();
    };
    let name = meta.get("sender_name").and_then(Value::as_str);
    let id = meta
        .get("sender_id")
        .or_else(|| meta.get("sender"))
        .and_then(Value::as_str);
    match (name, id) {
        (Some(name), Some(id)) => format!(" {name}（{id}）"),
        (Some(name), None) => format!(" {name}"),
        (None, Some(id)) => format!("（{id}）"),
        (None, None) => String::new(),
    }
}

/// 按字符截断，附上原长度提示
fn clip(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_MESSAGE_CHARS {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
    format!("{truncated}…（已截断，原 {total} 字符）")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, content: &str) -> Message {
        Message::User {
            time: "2026-11-01-10_00_00".to_string(),
            content: content.to_string(),
            meta: Some(serde_json::json!({"sender_id": id})),
        }
    }

    fn assistant(content: &str) -> Message {
        Message::Assistant {
            time: "2026-11-01-10_00_01".to_string(),
            content: content.to_string(),
            reasoning_content: Some("内部思考".to_string()),
            tool_calls: None,
            token_usage: Default::default(),
            model: "test".to_string(),
        }
    }

    #[test]
    fn formats_user_with_sender_and_hides_reasoning() {
        let out = format_message(&user("feishu:ou_1", "你好"), false);
        assert!(out.contains("feishu:ou_1"));
        assert!(out.contains("你好"));

        let out = format_message(&assistant("回复"), false);
        assert!(out.contains("回复"));
        assert!(!out.contains("内部思考"));

        let out = format_message(&assistant("回复"), true);
        assert!(out.contains("内部思考"));
    }

    #[test]
    fn clips_long_content() {
        let long = "字".repeat(MAX_MESSAGE_CHARS + 10);
        let out = clip(&long);
        assert!(out.contains("已截断"));
        assert_eq!(
            out.split('…').next().unwrap().chars().count(),
            MAX_MESSAGE_CHARS
        );
    }
}
