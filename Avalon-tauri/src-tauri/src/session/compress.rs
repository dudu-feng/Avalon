// 压缩输入渲染
//
// 压缩模型看到的是「干净的对话转录」，不是 SessionData 的存储 JSON：
// - 保留来源、附件观察、工具调用与结果；
// - 丢弃 time / token_usage / model / success / arguments 等元数据；
// - 工具结果使用比正常上下文更宽的预览预算，提高摘要质量。

use crate::session::context::render_user_content;
use crate::session::Message;

/// 压缩转录里单个工具结果的字符上限（比正常上下文更宽）
const COMPRESS_TOOL_PREVIEW_CHARS: usize = 4_000;

/// 把消息渲染成压缩模型可读的对话转录。
pub fn build_transcript(messages: &[Message]) -> String {
    let mut blocks: Vec<String> = Vec::new();

    for message in messages {
        match message {
            Message::User {
                content, meta, ..
            } => {
                let body = render_user_content(content, meta.as_ref());
                if !body.trim().is_empty() {
                    blocks.push(format!("[user]\n{body}"));
                }
            }
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let content = content.trim();
                if !content.is_empty() {
                    blocks.push(format!("[assistant]\n{content}"));
                }
                if let Some(calls) = tool_calls {
                    for call in calls {
                        let args = serde_json::to_string(&call.arguments)
                            .unwrap_or_else(|_| "{}".to_string());
                        blocks.push(format!("[assistant tool_calls]\n{}({args})", call.name));
                    }
                }
            }
            Message::Tool {
                tool_call_id,
                name,
                success,
                content,
                artifact,
                ..
            } => {
                let status = if *success { "" } else { "（失败）" };
                let preview = clip_chars(content, COMPRESS_TOOL_PREVIEW_CHARS);
                let mut block = format!("[tool result {tool_call_id}] {name}{status}\n{preview}");
                if let Some(artifact) = artifact {
                    block.push_str(&format!(
                        "\n[完整结果: artifact:{}（{} 字符）]",
                        artifact.id, artifact.chars
                    ));
                }
                blocks.push(block);
            }
        }
    }

    blocks.join("\n\n")
}

/// 压缩消息覆盖的时间范围（按 time 字段的日期部分）。
pub fn message_time_range(messages: &[Message]) -> Option<String> {
    let mut dates: Vec<String> = messages
        .iter()
        .filter_map(|m| date_part(message_time(m)))
        .collect();
    if dates.is_empty() {
        return None;
    }
    dates.sort();
    dates.dedup();
    let first = dates.first()?;
    let last = dates.last()?;
    if first == last {
        Some(first.clone())
    } else {
        Some(format!("{first} ~ {last}"))
    }
}

fn message_time(message: &Message) -> &str {
    match message {
        Message::User { time, .. } | Message::Assistant { time, .. } | Message::Tool { time, .. } => {
            time
        }
    }
}

/// time 形如 `2026-11-03-10:00:00`；取前 10 位并粗校验日期格式。
fn date_part(time: &str) -> Option<String> {
    let date = time.get(..10)?;
    let bytes = date.as_bytes();
    if bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        Some(date.to_string())
    } else {
        None
    }
}

/// 按字符截断，附原长度提示
fn clip_chars(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…（已截断，原 {total} 字符）")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{TokenUsage, ToolCall};

    fn user(content: &str) -> Message {
        Message::User {
            time: "2026-11-01-10:00:00".to_string(),
            content: content.to_string(),
            meta: Some(serde_json::json!({
                "sender_id": "feishu:ou_1",
                "sender_name": "张三",
                "chat_type": "group"
            })),
        }
    }

    #[test]
    fn transcript_keeps_source_and_tool_chain() {
        let messages = vec![
            user("看看这个文件"),
            Message::Assistant {
                time: "2026-11-01-10:00:01".to_string(),
                content: String::new(),
                reasoning_content: Some("内部思考不应出现".to_string()),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "a.txt"}),
                }]),
                token_usage: TokenUsage::default(),
                model: "test".to_string(),
            },
            Message::Tool {
                time: "2026-11-01-10:00:02".to_string(),
                tool_call_id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
                success: true,
                content: "文件内容".to_string(),
                artifact: Some(crate::session::ArtifactRef {
                    id: "call_1".to_string(),
                    chars: 12_000,
                    bytes: 36_000,
                }),
            },
        ];

        let text = build_transcript(&messages);
        assert!(text.contains("[user]"));
        assert!(text.contains("来源: 飞书"));
        assert!(text.contains("张三"));
        assert!(text.contains("read_file({\"path\":\"a.txt\"})"));
        assert!(text.contains("[tool result call_1] read_file"));
        assert!(text.contains("文件内容"));
        assert!(text.contains("[完整结果: artifact:call_1（12000 字符）]"));
        assert!(!text.contains("内部思考"));
        assert!(!text.contains("token_usage"));
    }

    #[test]
    fn long_tool_result_is_clipped() {
        let messages = vec![Message::Tool {
            time: "2026-11-01-10:00:00".to_string(),
            tool_call_id: "call_1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::Value::Null,
            success: false,
            content: "字".repeat(COMPRESS_TOOL_PREVIEW_CHARS + 10),
            artifact: None,
        }];
        let text = build_transcript(&messages);
        assert!(text.contains("（失败）"));
        assert!(text.contains("已截断"));
        assert!(text.chars().count() < COMPRESS_TOOL_PREVIEW_CHARS + 200);
    }

    #[test]
    fn time_range_spans_dates() {
        let messages = vec![
            Message::User {
                time: "2026-11-01-10:00:00".to_string(),
                content: "a".to_string(),
                meta: None,
            },
            Message::User {
                time: "2026-11-03-09:00:00".to_string(),
                content: "b".to_string(),
                meta: None,
            },
        ];
        assert_eq!(
            message_time_range(&messages).as_deref(),
            Some("2026-11-01 ~ 2026-11-03")
        );
    }
}
