// QQ 消息事件处理
//
// 与飞书 handler 同构：去重 → 准入 → 图片落盘 → 驱动 Engine → 输出。
// 差异集中在输出侧：QQ 没有交互卡片/表情接口，群聊只能发纯文本被动回复，
// 单聊才有原生流式，所以整个过程交给 out::QqOut 按会话类型分流。
//
// 外部渠道的图文通常分两条消息到达。纯图片先下载、回一句可配置的询问文本，
// 把图片挂在 PendingImages 里；下一条文字消息再一起送进 Engine。
// 这是为了不在用户还没提问时先跑一次视觉理解。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use super::api::QqApi;
use super::out::QqOut;
use super::proto::{self, GatewayEvent, IncomingMessage, Target};
use crate::channel::common::{sanitize_channel, Dedup, PendingImages, SlotRegistry, MAX_WAITING};
use crate::channel::contact::ContactStore;
use crate::config::{QqConfig, QqSessionMode};
use crate::engine::{Engine, EngineEvent, UserInput};
use crate::file::{FileSource, SaveRequest};
use crate::vision::ChatAttachment;

const DEDUP_TTL: std::time::Duration = std::time::Duration::from_secs(600);
const DEDUP_CAPACITY: usize = 1024;

/// 统一模式下所有 QQ 消息共用的会话名。与飞书的 feishu_unified 分开，
/// 避免两个渠道的上下文互相污染
const UNIFIED_CHANNEL: &str = "qq_unified";

pub struct MessageHandler {
    engine: Arc<Engine>,
    api: QqApi,
    config: QqConfig,
    /// 通讯录。只用于主人自动记录
    contacts: Arc<ContactStore>,
    dedup: Mutex<Dedup>,
    slots: SlotRegistry,
    pending_images: Mutex<PendingImages>,
}

impl MessageHandler {
    pub fn new(
        engine: Arc<Engine>,
        api: QqApi,
        config: QqConfig,
        contacts: Arc<ContactStore>,
    ) -> Self {
        Self {
            engine,
            api,
            config,
            contacts,
            dedup: Mutex::new(Dedup::new(DEDUP_TTL, DEDUP_CAPACITY)),
            slots: SlotRegistry::new(),
            pending_images: Mutex::new(PendingImages::default()),
        }
    }

    /// 处理一条网关事件。单条失败只记日志，不影响长连接
    pub async fn handle(&self, event: GatewayEvent) {
        if let Err(e) = self.try_handle(event).await {
            log::error!(target: "qq", "处理 QQ 消息失败: {e:#}");
        }
    }

    async fn try_handle(&self, event: GatewayEvent) -> Result<()> {
        if !proto::is_message_event(&event.t) {
            return Ok(());
        }

        let Some(msg) = proto::parse_message(&event.t, &event.d) else {
            log::debug!(target: "qq", "无法解析的 QQ 消息事件 {}，已忽略", event.t);
            return Ok(());
        };

        // 事件 id 缺失时退回 message_id，两者都空就不去重
        let dedup_key = event
            .id
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&msg.message_id);
        if !dedup_key.is_empty() && !self.dedup.lock().unwrap().accept(dedup_key) {
            log::debug!(target: "qq", "重复事件已忽略: {dedup_key}");
            return Ok(());
        }

        if msg.message_id.is_empty() {
            log::debug!(target: "qq", "QQ 消息缺少 message_id，无法被动回复，已忽略");
            return Ok(());
        }

        // —— 准入判定 ——

        if msg.is_group() && self.config.group_require_mention && !msg.mentioned_bot {
            return Ok(());
        }

        if !self.config.allow_users.is_empty()
            && !self.config.allow_users.contains(&msg.sender_open_id)
        {
            log::info!(target: "qq", "用户 {} 不在白名单，已忽略", msg.sender_open_id);
            return Ok(());
        }

        // 纯附件且没有可识别图片：语音、文件、视频都不支持，明确告知而不是装死
        if msg.images.is_empty() && msg.attachment_count > 0 {
            self.reply(&msg, "目前只能处理文本和图片消息哦。").await?;
            return Ok(());
        }

        if msg.text.trim().is_empty() && msg.images.is_empty() {
            return Ok(());
        }

        // 准入通过后才记主人：任意路人私聊一句不能把自己设成 owner
        self.record_owner(&msg);

        self.dispatch(msg).await
    }

    /// 纯图片先挂起并回一句询问，等用户补充文字
    async fn dispatch(&self, msg: IncomingMessage) -> Result<()> {
        let ack = self.config.image_ack_text.trim();
        let image_only = !msg.images.is_empty() && msg.text.trim().is_empty();
        if !image_only || ack.is_empty() {
            return self.run_engine(msg).await;
        }

        let pending_key = msg.pending_key();
        let mut saved = Vec::new();
        for attachment in &msg.images {
            match self
                .save_image(&msg, &attachment.url, &attachment.filename)
                .await
            {
                Ok(path) => saved.push(path),
                Err(e) => {
                    log::warn!(target: "qq", "图片下载失败: {e:#}");
                    self.reply(
                        &msg,
                        &format!("图片接收失败：{e:#}。可以重新发一次，或者直接用文字描述。"),
                    )
                    .await?;
                    return Ok(());
                }
            }
        }

        // 同一聊天连发多张只询问一次
        let ask = {
            let mut pending = self.pending_images.lock().unwrap();
            let mut ask = false;
            for path in saved {
                ask |= pending.push(pending_key.clone(), path, Instant::now());
            }
            ask
        };

        if ask {
            // 这条是主动发送的确认语，不进入 Engine，也占用一次被动回复
            if let Err(e) = self
                .api
                .send_text(
                    &Target::new(msg.chat_type, msg.chat_id.clone()),
                    ack,
                    &msg.message_id,
                    1,
                    None,
                )
                .await
            {
                log::warn!(target: "qq", "发送图片确认文本失败: {e:#}");
            }
        }
        Ok(())
    }

    /// 取走某会话待处理的图片
    fn take_pending(&self, channel: &str) -> Vec<ChatAttachment> {
        self.pending_images
            .lock()
            .unwrap()
            .take(channel, Instant::now())
            .into_iter()
            .map(|path| ChatAttachment { path })
            .collect()
    }

    /// 首次私聊时把发信人记为主人。只写 id + 昵称，画像归灵魂模块
    fn record_owner(&self, msg: &IncomingMessage) {
        if msg.is_group() || msg.sender_open_id.is_empty() {
            return;
        }
        let name = msg.sender_name.as_deref().unwrap_or("");
        match self.contacts.record_owner("qq", &msg.sender_open_id, name) {
            Ok(true) => log::info!(
                target: "qq",
                "已记录 QQ 主人到通讯录，qq_notify_owner 将发给该用户"
            ),
            Ok(false) => {}
            Err(e) => log::warn!(target: "qq", "记录 QQ 主人失败: {e:#}"),
        }
    }

    fn channel_key(&self, msg: &IncomingMessage) -> String {
        match self.config.session_mode {
            QqSessionMode::Unified => UNIFIED_CHANNEL.to_string(),
            QqSessionMode::Isolated => {
                sanitize_channel(&format!("qq_{}_{}", msg.chat_type.as_str(), msg.chat_id))
            }
        }
    }

    async fn run_engine(&self, msg: IncomingMessage) -> Result<()> {
        let channel = self.channel_key(&msg);

        // 同一会话串行。QQ 没有表情兜底，排队超限只能回一条文字
        let slot = self.slots.slot(&channel);
        if slot.waiting.fetch_add(1, Ordering::SeqCst) >= MAX_WAITING {
            slot.waiting.fetch_sub(1, Ordering::SeqCst);
            self.reply(&msg, "我这边排队的消息有点多，等我忙完这几条再来吧。")
                .await?;
            return Ok(());
        }

        let _guard = slot.lock.lock().await;
        slot.waiting.fetch_sub(1, Ordering::SeqCst);

        let pending = self.take_pending(&msg.pending_key());
        self.drive(&msg, &channel, pending).await
    }

    async fn save_image(&self, msg: &IncomingMessage, url: &str, filename: &str) -> Result<String> {
        let bytes = self.api.download_image(url).await?;
        let stored = self.engine.file_store().save_image(SaveRequest {
            bytes: &bytes,
            original_name: filename,
            declared_mime: None,
            source: FileSource::Qq,
            session_id: None,
            message_id: Some(&msg.message_id),
        })?;
        Ok(stored.path)
    }

    /// 跑一次 ReAct 并把输出交给 QqOut
    async fn drive(
        &self,
        msg: &IncomingMessage,
        channel: &str,
        pending: Vec<ChatAttachment>,
    ) -> Result<()> {
        self.engine.init_session(channel)?;
        let cancel: Arc<AtomicBool> = self.engine.begin_chat(channel);

        let mut history_text = if msg.text.trim().is_empty() {
            "发来一张图片".to_string()
        } else {
            msg.text.clone()
        };

        let mut attachments = pending;
        for attachment in &msg.images {
            match self
                .save_image(msg, &attachment.url, &attachment.filename)
                .await
            {
                Ok(path) => attachments.push(ChatAttachment { path }),
                Err(e) => {
                    // 不丢整条消息：把原因写进正文，用户和模型都知道发生了什么
                    history_text.push_str(&format!("\n\n[图片下载失败：{e:#}]"));
                }
            }
        }

        let hinted = msg.source_hint_with(&history_text);
        let input = UserInput {
            for_model: &hinted,
            for_history: &history_text,
            meta: Some(msg.meta()),
            attachments,
        };

        let target = Target::new(msg.chat_type, msg.chat_id.clone());
        let msg_id = msg.message_id.clone();
        let api = self.api.clone();
        let stream_enabled = !msg.is_group() && self.config.official.stream_c2c;

        // on_event 是同步闭包，所有网络推送都放到 pump 里
        let (tx, mut rx) = mpsc::unbounded_channel::<EngineEvent>();
        let pump = tauri::async_runtime::spawn(async move {
            let mut out = QqOut::new(api, target, msg_id, stream_enabled);
            while let Some(ev) = rx.recv().await {
                match ev {
                    EngineEvent::MessageDelta { delta } => out.push(&delta).await,
                    EngineEvent::Done { result } => out.set_fallback(&result.message),
                    _ => {}
                }
            }
            out
        });

        let run_result = self
            .engine
            .run_with_input(input, channel, cancel, move |ev| {
                let _ = tx.send(ev);
            })
            .await;

        // 闭包随 run 结束被 drop，tx 关闭后 pump 自然退出
        let out = pump.await.context("QQ 输出任务异常退出")?;
        let error_text = run_result.as_ref().err().map(|e| format!("{e:#}"));
        if let Some(e) = &error_text {
            log::error!(target: "qq", "ReAct 执行失败 channel={channel}: {e}");
        }

        let sent = out.finish(error_text.as_deref()).await;
        if !sent {
            log::warn!(target: "qq", "本次 QQ 回复没有成功发出任何内容 channel={channel}");
        }
        Ok(())
    }

    /// 发一条纯文本被动回复（seq=1）。只用于 Engine 之前的提示
    async fn reply(&self, msg: &IncomingMessage, text: &str) -> Result<()> {
        self.api
            .send_text(
                &Target::new(msg.chat_type, msg.chat_id.clone()),
                text,
                &msg.message_id,
                1,
                None,
            )
            .await
            .map(|_| ())
    }
}
