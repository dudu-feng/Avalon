// 飞书发送工具
//
// feishu_notify_owner → 发给配置里的主人（open_id）
// feishu_send_to      → 发到指定会话（chat_id）
//
// 这两个工具补的是「产出没有出口」这个洞：定时任务跑在 task_xxx 会话上，
// 事件回调是空的，结果只进会话文件。有了它们，无人值守的产出才能推到用户手机上。
//
// 只做发送，不做「回复当前会话」：飞书对话的正文回复由 stream.rs 自动发送，
// 模型再调一次就会双发。工具描述里必须说清这件事。

use std::sync::Arc;

use serde_json::Value;

use crate::channel::{FeishuHandle, Target};
use crate::config::ConfigStore;

/// 取字符串参数并去空白，缺失或空串返回 None
fn arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 发给主人。收件人来自配置，模型无法指定 —— 这是它唯一能主动打扰用户的通道
pub async fn notify_owner(
    args: &Value,
    handle: &Arc<FeishuHandle>,
    config: Option<&ConfigStore>,
) -> String {
    let Some(text) = arg(args, "text") else {
        return "参数错误: 缺少 text 或内容为空".to_string();
    };

    // owner 现读而不是缓存：自动填充改的是同一个 ConfigStore，下次调用自然拿到新值
    let owner = config
        .map(|c| c.get().feishu.owner_open_id)
        .unwrap_or_default();
    if owner.is_empty() {
        return "尚未记录主人的 open_id。请先私聊一次机器人，或在设置页「渠道」里填写 owner_open_id"
            .to_string();
    }

    match handle.send(&Target::User(owner), &text).await {
        Ok(id) => {
            log::info!(target: "feishu", "已向主人推送消息 {id}");
            "已发送给主人".to_string()
        }
        Err(e) => {
            log::warn!(target: "feishu", "向主人推送失败: {e:#}");
            format!("发送失败: {e}")
        }
    }
}

/// 发到指定会话。只收 chat_id：open_id 等于允许给组织里任意人发私信，
/// 而 chat_id 限于机器人被拉进过的会话，爆炸半径小得多
pub async fn send_to(args: &Value, handle: &Arc<FeishuHandle>) -> String {
    let Some(chat_id) = arg(args, "chat_id") else {
        return "参数错误: 缺少 chat_id 或内容为空".to_string();
    };
    let Some(text) = arg(args, "text") else {
        return "参数错误: 缺少 text 或内容为空".to_string();
    };
    // ou_ 是用户 open_id，发到这里会被飞书当成会话 id 而失败，提前给出可读原因
    if chat_id.starts_with("ou_") {
        return "参数错误: chat_id 应是会话 id（oc_ 开头）。要发给某个人请用 feishu_notify_owner"
            .to_string();
    }

    match handle.send(&Target::Chat(chat_id.clone()), &text).await {
        Ok(id) => {
            log::info!(target: "feishu", "已向会话 {chat_id} 发送消息 {id}");
            format!("已发送到会话 {chat_id}")
        }
        Err(e) => {
            log::warn!(target: "feishu", "向会话 {chat_id} 发送失败: {e:#}");
            format!("发送失败: {e}")
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
        let out =
            tauri::async_runtime::block_on(notify_owner(&json!({ "text": "   " }), &handle(), None));
        assert!(out.contains("缺少 text"));
    }

    #[test]
    fn 未配置主人时提示如何设置() {
        let out =
            tauri::async_runtime::block_on(notify_owner(&json!({ "text": "hi" }), &handle(), None));
        assert!(out.contains("owner_open_id"));
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
}
