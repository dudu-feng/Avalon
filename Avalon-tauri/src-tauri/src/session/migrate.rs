// 旧会话数据清洗
//
// 新写侧保证一次 run 的消息在内存累积、成功结束时整轮原子提交，取消的轮次整轮丢弃。
// 旧版本在停止时仍会把已生成的部分轨迹写入 current，可能留下
// assistant(tool_calls) 缺少对应 tool 结果的半条工具链。
//
// 模型上下文投影已经不做运行时配对修复（见 context::build_history_messages），
// 因此这类历史数据需要在启动时清洗一次。

use std::collections::HashSet;

use crate::session::Message;

/// 清理不完整工具链，返回删除的消息数。
///
/// 规则：
/// - assistant 声明的每个 tool_call，必须在紧随其后的连续 tool 消息里都有结果；
///   缺任何一个 → 整组删除（assistant + 已落盘的部分结果）。
/// - 多余或重复的 tool 结果（id 未声明 / 已出现）单独删除。
/// - 没有前置 assistant 的孤立 tool 消息删除。
///
/// 函数幂等：清洗过的消息再调用一次返回 0。
pub fn sanitize_tool_chains(messages: &mut Vec<Message>) -> usize {
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut removed = 0usize;
    let mut i = 0usize;

    while i < messages.len() {
        match &messages[i] {
            Message::Assistant {
                tool_calls: Some(calls),
                ..
            } if !calls.is_empty() => {
                // 存储保证 assistant 的 tool 结果紧跟在后面；先把连续的 tool 段圈出来
                let mut j = i + 1;
                while j < messages.len() && matches!(messages[j], Message::Tool { .. }) {
                    j += 1;
                }

                let expected: HashSet<&str> = calls.iter().map(|c| c.id.as_str()).collect();
                let results: HashSet<&str> = messages[i + 1..j]
                    .iter()
                    .filter_map(|m| match m {
                        Message::Tool { tool_call_id, .. } => Some(tool_call_id.as_str()),
                        _ => None,
                    })
                    .collect();

                if expected.is_subset(&results) {
                    out.push(messages[i].clone());
                    let mut seen: HashSet<&str> = HashSet::new();
                    for tool_msg in &messages[i + 1..j] {
                        let Message::Tool { tool_call_id, .. } = tool_msg else {
                            continue;
                        };
                        if expected.contains(tool_call_id.as_str())
                            && seen.insert(tool_call_id.as_str())
                        {
                            out.push(tool_msg.clone());
                        } else {
                            removed += 1;
                        }
                    }
                } else {
                    // 结果不齐：整组丢弃，避免把半个工具链交给模型
                    removed += j - i;
                }
                i = j;
            }
            Message::Tool { .. } => {
                removed += 1;
                i += 1;
            }
            _ => {
                out.push(messages[i].clone());
                i += 1;
            }
        }
    }

    *messages = out;
    removed
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::llm::ToolCall;

    fn user(content: &str) -> Message {
        Message::User {
            time: "2026-11-03-10:00:00".to_string(),
            content: content.to_string(),
            meta: None,
        }
    }

    fn assistant(content: &str, ids: &[&str]) -> Message {
        Message::Assistant {
            time: "2026-11-03-10:00:01".to_string(),
            content: content.to_string(),
            reasoning_content: None,
            tool_calls: (!ids.is_empty()).then(|| {
                ids.iter()
                    .map(|id| ToolCall {
                        id: (*id).to_string(),
                        name: "read_file".to_string(),
                        arguments: json!({"path": "a.txt"}),
                    })
                    .collect()
            }),
            token_usage: Default::default(),
            model: "test".to_string(),
        }
    }

    fn tool(id: &str, content: &str) -> Message {
        Message::Tool {
            time: "2026-11-03-10:00:02".to_string(),
            tool_call_id: id.to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path": "a.txt"}),
            success: true,
            content: content.to_string(),
            artifact: None,
        }
    }

    #[test]
    fn complete_chain_is_kept() {
        let mut messages = vec![
            user("开始"),
            assistant("", &["call_1", "call_2"]),
            tool("call_1", "一"),
            tool("call_2", "二"),
            assistant("结束", &[]),
        ];
        assert_eq!(sanitize_tool_chains(&mut messages), 0);
        assert_eq!(messages.len(), 5);
    }

    #[test]
    fn partial_chain_is_dropped_as_a_whole() {
        let mut messages = vec![
            user("开始"),
            assistant("", &["call_1", "call_2"]),
            tool("call_1", "只有一半"),
            assistant("结束", &[]),
        ];
        assert_eq!(sanitize_tool_chains(&mut messages), 2);
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0], Message::User { .. }));
        let Message::Assistant { content, .. } = &messages[1] else {
            panic!("第二条应为 assistant");
        };
        assert_eq!(content, "结束");
    }

    #[test]
    fn orphan_and_duplicate_tools_are_dropped() {
        let mut messages = vec![
            tool("call_orphan", "孤立结果"),
            user("开始"),
            assistant("", &["call_1"]),
            tool("call_1", "结果"),
            tool("call_1", "重复结果"),
            tool("call_other", "未声明结果"),
            assistant("结束", &[]),
        ];
        assert_eq!(sanitize_tool_chains(&mut messages), 3);
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| match m {
                Message::User { .. } => "user",
                Message::Assistant { .. } => "assistant",
                Message::Tool { .. } => "tool",
            })
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "tool", "assistant"]);
    }

    #[test]
    fn sanitize_is_idempotent() {
        let mut messages = vec![
            user("开始"),
            assistant("", &["call_1"]),
            tool("call_1", "结果"),
        ];
        assert_eq!(sanitize_tool_chains(&mut messages), 0);
        let snapshot = serde_json::to_value(&messages).unwrap();
        assert_eq!(sanitize_tool_chains(&mut messages), 0);
        assert_eq!(serde_json::to_value(&messages).unwrap(), snapshot);
    }
}
