// QQ 发送工具
//
// qq_notify_owner → 通过 QQ 单聊主动给主人发一条消息
// qq_send_to      → 发到指定 QQ 群聊或单聊，可选引用某条消息
//
// 与飞书工具的分工一致：QQ 对话里的正文回复由 out.rs / stream 自动发送，
// 这里的工具只用于定时任务、后台任务等「当前没有对话出口」的场景。
// 主人 id 的核对交给通讯录 get_owner_contact，不再单独维护查询工具。
//
// QQ 的主动消息比被动回复贵：平台对主动消息有额度限制，通常要求用户
// 近期与机器人有过互动。工具描述与失败提示都必须把这一点讲清楚，避免模型
// 把额度耗尽当成偶发网络错误反复重试。

use std::sync::Arc;

use serde_json::Value;

use crate::channel::contact::ContactStore;
use crate::channel::qq::official::proto::{ChatType, Target as QqTarget};
use crate::channel::QqHandle;

/// 取字符串参数并去空白，兼容常见正文别名
fn arg(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        args.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn text_arg(args: &Value) -> Option<String> {
    arg(args, &["text", "content", "message", "msg"])
}

/// 参数错误时记录字段名（不记正文），便于定位模型字段名写错还是引擎丢参数
fn log_bad_args(tool: &str, args: &Value) {
    let keys: Vec<&str> = args
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    log::warn!(target: "qq", "{tool} 参数错误：缺少必需参数，收到字段 {keys:?}");
}

/// 读通讯录里的 QQ 主人裸 openid（去掉 qq: 前缀）
fn owner_id(contacts: Option<&ContactStore>) -> String {
    contacts
        .and_then(|c| c.owner_open_id("qq"))
        .unwrap_or_default()
}

/// 主动给主人发一条 QQ 单聊消息
pub async fn notify_owner(
    args: &Value,
    handle: &Arc<QqHandle>,
    contacts: Option<&ContactStore>,
) -> String {
    let Some(text) = text_arg(args) else {
        log_bad_args("qq_notify_owner", args);
        return "参数错误: 缺少 text 或内容为空".to_string();
    };

    let owner = owner_id(contacts);
    if owner.is_empty() {
        return "尚未记录 QQ 主人。请先私聊一次机器人（会自动记录），\
                也可在设置 → 通讯录里补填；还可用 get_owner_contact 核对当前记录"
            .to_string();
    }

    match handle.send_to_owner(&owner, &text).await {
        Ok(id) => {
            log::info!(target: "qq", "已向主人推送主动消息 {id}");
            "已发送给主人".to_string()
        }
        Err(e) => {
            log::warn!(target: "qq", "向主人推送失败: {e:#}");
            let message = format!("发送失败: {e}\n渠道状态：{}", handle.status());
            if message.contains("未运行") {
                return message;
            }
            format!(
                "{message}\n\
                 注意：QQ 主动消息受平台额度限制，通常需要主人近期与机器人互动过；\
                 额度不足时请改为等待用户来消息后再回复，或缩短为一条消息"
            )
        }
    }
}

/// 发到指定 QQ 会话。
///
/// chat_type 必须显式给出：群 openid 和用户 openid 都是不透明字符串，
/// 只看 id 无法判断该走 `/v2/groups/...` 还是 `/v2/users/...`。
pub async fn send_to(args: &Value, handle: &Arc<QqHandle>) -> String {
    let Some(chat_type) = arg(args, &["chat_type", "type"]) else {
        log_bad_args("qq_send_to", args);
        return "参数错误: 缺少 chat_type（group 或 c2c）".to_string();
    };
    let normalized = chat_type.to_ascii_lowercase();
    let chat_type = match normalized.as_str() {
        "group" => ChatType::Group,
        "c2c" | "private" | "user" | "p2p" => ChatType::C2c,
        _ => {
            return format!("参数错误: chat_type 只能是 group 或 c2c（收到 {chat_type}）");
        }
    };

    let Some(chat_id) = arg(args, &["chat_id", "open_id", "group_openid", "user_openid"]) else {
        log_bad_args("qq_send_to", args);
        return "参数错误: 缺少 chat_id 或内容为空".to_string();
    };
    let Some(text) = text_arg(args) else {
        log_bad_args("qq_send_to", args);
        return "参数错误: 缺少 text 或内容为空".to_string();
    };

    // 引用不是必填：QQ 要的是 msg_idx（REFIDX_ 开头），不是 message.id
    let quote = arg(args, &["quote_message_id", "reply_to", "quote_id"]);
    let target = QqTarget::new(chat_type, chat_id.clone());
    match handle.send_to(&target, &text, quote.as_deref()).await {
        Ok(id) => {
            log::info!(target: "qq", "已向 QQ 会话 {chat_id}（{}）发送消息 {id}", chat_type.as_str());
            match quote {
                Some(message_id) => format!(
                    "已发送到 QQ 会话 {chat_id}（{}，引用消息 {message_id}）",
                    chat_type.as_str()
                ),
                None => format!("已发送到 QQ 会话 {chat_id}（{}）", chat_type.as_str()),
            }
        }
        Err(e) => {
            log::warn!(target: "qq", "向 QQ 会话 {chat_id} 发送失败: {e:#}");
            let message = format!("发送失败: {e}\n渠道状态：{}", handle.status());
            if message.contains("未运行") {
                return message;
            }
            format!(
                "{message}\n\
                 注意：QQ 主动消息受平台额度限制，通常需要该会话近期与机器人互动过；\
                 额度不足时请改为等待用户来消息后再回复，或合并为一条消息"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn handle() -> Arc<QqHandle> {
        Arc::new(QqHandle::new())
    }

    #[test]
    fn 缺少text时不发送并说明原因() {
        let out = tauri::async_runtime::block_on(notify_owner(&json!({}), &handle(), None));
        assert!(out.contains("缺少 text"));
    }

    #[test]
    fn 空白text视为缺失() {
        let out = tauri::async_runtime::block_on(notify_owner(
            &json!({ "text": "   " }),
            &handle(),
            None,
        ));
        assert!(out.contains("缺少 text"));
    }

    #[test]
    fn 兼容message别名() {
        let out = tauri::async_runtime::block_on(notify_owner(
            &json!({ "message": "hi" }),
            &handle(),
            None,
        ));
        assert!(out.contains("尚未记录 QQ 主人"));
        assert!(!out.contains("缺少 text"));
    }

    #[test]
    fn 未记录主人时提示如何设置() {
        let out =
            tauri::async_runtime::block_on(notify_owner(&json!({ "text": "hi" }), &handle(), None));
        assert!(out.contains("尚未记录 QQ 主人"));
        assert!(out.contains("通讯录"));
    }

    fn contacts_with_owner(owner: &str) -> ContactStore {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "avalon-qq-owner-test-{}-{stamp}.json",
            std::process::id()
        ));
        let store = ContactStore::new(path);
        store.record_owner("qq", owner, "").unwrap();
        store
    }

    #[test]
    fn send_to缺少chat_type时报参数错误() {
        let out = tauri::async_runtime::block_on(send_to(
            &json!({ "chat_id": "G1", "text": "hi" }),
            &handle(),
        ));
        assert!(out.contains("缺少 chat_type"));
    }

    #[test]
    fn send_to拒绝未知chat_type() {
        let out = tauri::async_runtime::block_on(send_to(
            &json!({ "chat_type": "guild", "chat_id": "G1", "text": "hi" }),
            &handle(),
        ));
        assert!(out.contains("只能是 group 或 c2c"));
    }

    #[test]
    fn send_to缺少chat_id时报参数错误() {
        let out = tauri::async_runtime::block_on(send_to(
            &json!({ "chat_type": "group", "text": "hi" }),
            &handle(),
        ));
        assert!(out.contains("缺少 chat_id"));
    }

    #[test]
    fn send_to兼容private别名并落到c2c() {
        let out = tauri::async_runtime::block_on(send_to(
            &json!({ "chat_type": "private", "chat_id": "U1", "text": "hi" }),
            &handle(),
        ));
        // 渠道离线，但参数解析已通过并走到发送阶段
        assert!(out.contains("未运行"));
    }

    #[test]
    fn send_to群聊离线时返回未运行() {
        let out = tauri::async_runtime::block_on(send_to(
            &json!({ "chat_type": "group", "chat_id": "G1", "text": "hi" }),
            &handle(),
        ));
        assert!(out.contains("发送失败"));
        assert!(out.contains("未运行"));
    }

    #[test]
    fn send_to带引用时把msg_idx透传到发送层() {
        let out = tauri::async_runtime::block_on(send_to(
            &json!({
                "chat_type": "group",
                "chat_id": "G1",
                "text": "hi",
                "quote_message_id": "REFIDX_abc"
            }),
            &handle(),
        ));
        // 渠道离线，参数解析与引用透传都已通过，走到发送阶段
        assert!(out.contains("发送失败"));
        assert!(out.contains("未运行"));
    }

    #[test]
    fn 有主人但渠道离线时发送返回未运行() {
        let contacts = contacts_with_owner("U_abc");
        let out = tauri::async_runtime::block_on(notify_owner(
            &json!({ "text": "hi" }),
            &handle(),
            Some(&contacts),
        ));
        assert!(out.contains("发送失败"));
        assert!(out.contains("未运行"));
    }
}
