// 飞书发送工具
//
// feishu_notify_owner → 发给配置里的主人（open_id）
// feishu_send_to      → 发到指定会话（chat_id），可选引用某条消息
//
// 这两个工具补的是「产出没有出口」这个洞：定时任务跑在 task_xxx 会话上，
// 事件回调是空的，结果只进会话文件。有了它们，无人值守的产出才能推到用户手机上。
// 主人 id 的核对交给通讯录 get_owner_contact，不再单独维护查询工具。
//
// 只做发送，不做「回复当前会话」：飞书对话的正文回复由 stream.rs 自动发送，
// 模型再调一次就会双发。工具描述里必须说清这件事。

use std::sync::Arc;

use serde_json::Value;

use crate::channel::contact::ContactStore;
use crate::channel::{FeishuHandle, Target};

/// 取字符串参数并去空白，缺失或空串返回 None。
///
/// 除标准字段名外兼容常见别名：不同模型会把正文写成 content / message，
/// 严格只认 text 会让一次本来能成功的发送变成「参数错误」。
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

/// 参数错误时把「模型到底传了哪些字段」写进运行日志，便于事后定位是字段名
/// 写错还是引擎没把参数传下来 —— 日志里只记字段名，不记正文内容。
fn log_bad_args(tool: &str, args: &Value) {
    let keys: Vec<&str> = args
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    log::warn!(target: "feishu", "{tool} 参数错误：缺少必需参数，收到字段 {keys:?}");
}

/// 读通讯录里的飞书主人裸 open_id（去掉 feishu: 前缀）
fn owner_id(contacts: Option<&ContactStore>) -> String {
    contacts
        .and_then(|c| c.owner_open_id("feishu"))
        .unwrap_or_default()
}

/// 发给主人。收件人来自通讯录，模型无法指定 —— 这是它唯一能主动打扰用户的通道
pub async fn notify_owner(
    args: &Value,
    handle: &Arc<FeishuHandle>,
    contacts: Option<&ContactStore>,
) -> String {
    let Some(text) = text_arg(args) else {
        log_bad_args("feishu_notify_owner", args);
        return "参数错误: 缺少 text 或内容为空".to_string();
    };

    // owner 现读而不是缓存：渠道首次私聊会自动写入同一个 ContactStore
    let owner = owner_id(contacts);
    if owner.is_empty() {
        return "尚未记录飞书主人。请先私聊一次机器人（会自动记录），\
                也可在设置 → 通讯录里补填；还可用 get_owner_contact 核对当前记录"
            .to_string();
    }
    if !owner.starts_with("ou_") {
        return format!(
            "通讯录里的飞书主人 id 格式不正确：应以 ou_ 开头（当前为 {owner}）。\
             请在设置 → 通讯录里更正，或调用 get_owner_contact 查看"
        );
    }

    match handle.send(&Target::User(owner), &text).await {
        Ok(id) => {
            log::info!(target: "feishu", "已向主人推送消息 {id}");
            "已发送给主人".to_string()
        }
        Err(e) => {
            log::warn!(target: "feishu", "向主人推送失败: {e:#}");
            format!("发送失败: {e}\n渠道状态：{}", handle.status())
        }
    }
}

/// 发到指定会话。只收 chat_id：open_id 等于允许给组织里任意人发私信，
/// 而 chat_id 限于机器人被拉进过的会话，爆炸半径小得多
pub async fn send_to(args: &Value, handle: &Arc<FeishuHandle>) -> String {
    let Some(chat_id) = arg(args, &["chat_id"]) else {
        return "参数错误: 缺少 chat_id 或内容为空".to_string();
    };
    let Some(text) = text_arg(args) else {
        log_bad_args("feishu_send_to", args);
        return "参数错误: 缺少 text 或内容为空".to_string();
    };
    // ou_ 是用户 open_id，发到这里会被飞书当成会话 id 而失败，提前给出可读原因
    if chat_id.starts_with("ou_") {
        return "参数错误: chat_id 应是会话 id（oc_ 开头）。要发给某个人请用 feishu_notify_owner"
            .to_string();
    }

    // 引用不是必填：带 quote_message_id 时走回复端点，否则普通发送
    let quote = arg(args, &["quote_message_id", "reply_to", "quote_id"]);
    let result = match quote.as_deref() {
        Some(message_id) => handle.send_reply(message_id, &text).await,
        None => handle.send(&Target::Chat(chat_id.clone()), &text).await,
    };

    match result {
        Ok(id) => {
            log::info!(target: "feishu", "已向会话 {chat_id} 发送消息 {id}");
            match quote {
                Some(message_id) => format!("已发送到会话 {chat_id}（引用消息 {message_id}）"),
                None => format!("已发送到会话 {chat_id}"),
            }
        }
        Err(e) => {
            log::warn!(target: "feishu", "向会话 {chat_id} 发送失败: {e:#}");
            format!("发送失败: {e}\n渠道状态：{}", handle.status())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn handle() -> Arc<FeishuHandle> {
        Arc::new(FeishuHandle::new())
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
    fn 兼容content别名() {
        // 没有配置主人时也会先通过参数校验，落到 owner 检查而不是 text 检查
        let out = tauri::async_runtime::block_on(notify_owner(
            &json!({ "content": "hi" }),
            &handle(),
            None,
        ));
        assert!(out.contains("尚未记录飞书主人"));
        assert!(!out.contains("缺少 text"));
    }

    #[test]
    fn 未记录主人时提示如何设置() {
        let out =
            tauri::async_runtime::block_on(notify_owner(&json!({ "text": "hi" }), &handle(), None));
        assert!(out.contains("尚未记录飞书主人"));
        assert!(out.contains("通讯录"));
    }

    #[test]
    fn send_to拒绝用户open_id并指引到正确工具() {
        let args = json!({ "chat_id": "ou_abc", "text": "hi" });
        let out = tauri::async_runtime::block_on(send_to(&args, &handle()));
        assert!(out.contains("feishu_notify_owner"));
    }

    #[test]
    fn send_to缺少chat_id时报参数错误() {
        let out = tauri::async_runtime::block_on(send_to(&json!({ "text": "hi" }), &handle()));
        assert!(out.contains("缺少 chat_id"));
    }

    #[test]
    fn 渠道离线时返回可读错误而非panic() {
        let args = json!({ "chat_id": "oc_abc", "text": "hi" });
        let out = tauri::async_runtime::block_on(send_to(&args, &handle()));
        assert!(out.contains("发送失败"));
        assert!(out.contains("未运行"));
    }

    #[test]
    fn send_to带引用时走回复端点并在离线时返回可读错误() {
        let args = json!({
            "chat_id": "oc_abc",
            "text": "hi",
            "quote_message_id": "om_1"
        });
        let out = tauri::async_runtime::block_on(send_to(&args, &handle()));
        assert!(out.contains("发送失败"));
        assert!(out.contains("未运行"));
    }

    fn contacts_with_owner(owner: &str) -> ContactStore {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "avalon-feishu-owner-test-{}-{stamp}.json",
            std::process::id()
        ));
        let store = ContactStore::new(path);
        store.record_owner("feishu", owner, "").unwrap();
        store
    }

    #[test]
    fn 有主人但渠道离线时发送返回未运行() {
        let contacts = contacts_with_owner("ou_abc");
        let out = tauri::async_runtime::block_on(notify_owner(
            &json!({ "text": "hi" }),
            &handle(),
            Some(&contacts),
        ));
        assert!(out.contains("发送失败"));
        assert!(out.contains("未运行"));
    }

    #[test]
    fn 通讯录里的主人id格式不对时给出可读提示() {
        let contacts = contacts_with_owner("not_an_open_id");
        let out = tauri::async_runtime::block_on(notify_owner(
            &json!({ "text": "hi" }),
            &handle(),
            Some(&contacts),
        ));
        assert!(out.contains("应以 ou_ 开头"));
        assert!(out.contains("通讯录"));
    }
}
