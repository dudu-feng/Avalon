// 旧会话数据清洗
//
// 新写侧保证一次 run 的消息在内存累积、成功结束时整轮原子提交，取消的轮次整轮丢弃。
// 旧版本在停止时仍会把已生成的部分轨迹写入 current，可能留下
// assistant(tool_calls) 缺少对应 tool 结果的半条工具链。
//
// 模型上下文投影已经不做运行时配对修复（见 context::build_history_messages），
// 因此这类历史数据需要在启动时清洗一次。

use std::collections::HashSet;

use crate::session::{Message, MessageOrigin, MessageSource, TrustLevel};

/// 统一会话迁移统计（启动日志用）
#[derive(Debug, Default, Clone, Copy)]
pub struct MigrationReport {
    /// 成功归档的旧 current 数
    pub archived: usize,
    /// 合并进 main 的未压缩消息数
    pub merged_messages: usize,
    /// 因 token 预算未合并的轮次数
    pub skipped_turns: usize,
    /// 解析/写入失败的旧 current 数
    pub failures: usize,
}

impl MigrationReport {
    pub fn is_clean(&self) -> bool {
        self.failures == 0
    }
}

/// 把一个旧 current 的未压缩消息按轮次分组：user 开启一组，后续 assistant/tool 属于同组。
pub fn group_turns(messages: &[Message]) -> Vec<Vec<Message>> {
    let mut groups: Vec<Vec<Message>> = Vec::new();
    for message in messages {
        if matches!(message, Message::User { .. }) || groups.is_empty() {
            groups.push(vec![message.clone()]);
        } else if let Some(last) = groups.last_mut() {
            last.push(message.clone());
        }
    }
    groups
}

/// 回填一组消息的 origin：
/// - user 缺失时按桌面主人补齐；
/// - assistant/tool 缺失时继承该轮 user 的 origin；
/// - 旧外部消息的 trust 保持 Unknown，避免误标「非主人」。
pub fn fill_group_origin(group: &mut [Message]) {
    let group_origin = group
        .iter()
        .find_map(|m| match m {
            Message::User { meta, .. } => meta.clone(),
            _ => None,
        })
        .unwrap_or_else(MessageOrigin::desktop_owner);

    for message in group.iter_mut() {
        match message {
            Message::User { meta, .. } => {
                if meta.is_none() {
                    *meta = Some(MessageOrigin::desktop_owner());
                }
            }
            Message::Assistant { origin, .. } | Message::Tool { origin, .. } => {
                if origin.is_none() {
                    *origin = Some(group_origin.clone());
                }
            }
        }
    }
}

/// 合并去重键：来源 + 时间 + 角色 + 正文。
pub fn dedup_key(message: &Message) -> (MessageSource, String, &'static str, String) {
    match message {
        Message::User {
            time,
            content,
            meta,
        } => {
            let source = meta
                .as_ref()
                .map(|m| m.source)
                .unwrap_or(MessageSource::Desktop);
            (source, time.clone(), "user", content.clone())
        }
        Message::Assistant {
            time,
            content,
            origin,
            ..
        } => {
            let source = origin
                .as_ref()
                .map(|m| m.source)
                .unwrap_or(MessageSource::Desktop);
            (source, time.clone(), "assistant", content.clone())
        }
        Message::Tool {
            time,
            content,
            origin,
            ..
        } => {
            let source = origin
                .as_ref()
                .map(|m| m.source)
                .unwrap_or(MessageSource::Desktop);
            (source, time.clone(), "tool", content.clone())
        }
    }
}

/// 旧数据里外部来源缺少可信级别时，统一落成 Unknown；任务/桌面按主人。
pub fn normalize_legacy_trust(origin: &mut MessageOrigin) {
    if origin.trust != TrustLevel::Unknown {
        return;
    }
    if matches!(
        origin.source,
        MessageSource::Desktop | MessageSource::Task
    ) {
        origin.trust = TrustLevel::Owner;
    }
}

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
            origin: None,
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
            origin: None,
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
