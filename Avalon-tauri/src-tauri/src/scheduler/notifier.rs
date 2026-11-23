// 定时任务结果通知
//
// run 结束后由 Scheduler 调用：按任务的 notify_channel 把最终回复投递到桌面或渠道。
// 通知失败只记日志 / 写 notify_error，不影响任务本身的成功状态。

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use tauri::{AppHandle, Emitter};

use crate::channel::contact::ContactStore;
use crate::channel::feishu::api::Target as FeishuTarget;
use crate::channel::qq::official::proto::{ChatType, Target as QqTarget};
use crate::channel::{FeishuHandle, QqHandle};

use super::TaskNotifyChannel;

/// 通知发送契约。Scheduler 只依赖这个 trait，便于测试替换。
#[async_trait]
pub trait TaskNotifier: Send + Sync {
    async fn notify(
        &self,
        channel: TaskNotifyChannel,
        target: Option<&str>,
        title: &str,
        body: &str,
    ) -> Result<()>;
}

/// 默认实现：桌面走 Tauri 事件；飞书 / QQ 复用渠道发送句柄，缺省发给主人。
pub struct DefaultTaskNotifier {
    app: AppHandle,
    feishu: Arc<FeishuHandle>,
    qq: Arc<QqHandle>,
    contacts: Arc<ContactStore>,
}

impl DefaultTaskNotifier {
    pub fn new(
        app: AppHandle,
        feishu: Arc<FeishuHandle>,
        qq: Arc<QqHandle>,
        contacts: Arc<ContactStore>,
    ) -> Self {
        Self {
            app,
            feishu,
            qq,
            contacts,
        }
    }
}

#[async_trait]
impl TaskNotifier for DefaultTaskNotifier {
    async fn notify(
        &self,
        channel: TaskNotifyChannel,
        target: Option<&str>,
        title: &str,
        body: &str,
    ) -> Result<()> {
        match channel {
            TaskNotifyChannel::None => Ok(()),
            TaskNotifyChannel::Desktop => {
                self.app
                    .emit(
                        "task-notification",
                        serde_json::json!({
                            "title": title,
                            "body": body,
                        }),
                    )
                    .map_err(|e| anyhow!("发送桌面通知失败: {e}"))
            }
            TaskNotifyChannel::Feishu => {
                let target = match target.map(str::trim).filter(|t| !t.is_empty()) {
                    Some(id) if id.starts_with("oc_") => FeishuTarget::Chat(id.to_string()),
                    Some(id) => FeishuTarget::User(strip_prefix(id, "feishu:")),
                    None => {
                        let id = self
                            .contacts
                            .owner_open_id("feishu")
                            .ok_or_else(|| anyhow!("通讯录还没有飞书主人，无法发送通知"))?;
                        FeishuTarget::User(id)
                    }
                };
                self.feishu.send(&target, body).await.map(|_| ())
            }
            TaskNotifyChannel::Qq => {
                let raw = match target.map(str::trim).filter(|t| !t.is_empty()) {
                    Some(id) => id.to_string(),
                    None => self
                        .contacts
                        .owner_open_id("qq")
                        .ok_or_else(|| anyhow!("通讯录还没有 QQ 主人，无法发送通知"))?,
                };
                let (chat_type, id) = match raw.strip_prefix("group:") {
                    Some(id) => (ChatType::Group, id.to_string()),
                    None => (ChatType::C2c, strip_prefix(&raw, "qq:")),
                };
                if id.is_empty() {
                    bail!("QQ 通知目标为空");
                }
                self.qq
                    .send_to(&QqTarget::new(chat_type, id), body, None)
                    .await
                    .map(|_| ())
            }
        }
    }
}

fn strip_prefix(raw: &str, prefix: &str) -> String {
    raw.strip_prefix(prefix).unwrap_or(raw).to_string()
}
