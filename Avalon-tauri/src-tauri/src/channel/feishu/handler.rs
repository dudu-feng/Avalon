// 飞书消息事件处理
//
// 从 ws.rs 收到的原始事件 JSON 出发：解析 → 去重 → 准入判定 → 打「处理中」表情 →
// 驱动 Engine::run（正文走消息、过程走卡片，见 stream.rs）→ 打「完成」表情。
//
// 每条消息独立 spawn，彼此不阻塞；同一会话内的串行由本文件的 ChannelSlot 锁保证。
// 之所以要自己去重：ws.rs 是「收到就 ACK」，但网络抖动、服务端重投都可能让同一个
// event_id 到达两次，没有去重就会把同一句话跑两遍 ReAct。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::api::FeishuApi;
use super::reaction::{ReactionGate, ReactionTracker};
use super::stream;
use crate::channel::common::{sanitize_channel, Dedup, PendingImages, SlotRegistry, MAX_WAITING};
use crate::config::{ConfigStore, FeishuConfig, FeishuSessionMode};
use crate::engine::{Engine, EngineEvent, UserInput};
use crate::file::{FileSource, SaveRequest};
use crate::vision::ChatAttachment;

/// 已处理 event_id 的保留时长
const DEDUP_TTL: Duration = Duration::from_secs(600);
/// 去重表容量上限，超出后清理最旧的一半，防止长期运行内存无限增长
const DEDUP_CAPACITY: usize = 1024;

/// 统一模式下所有飞书消息共用的会话名。
/// 刻意避开裸 "feishu" —— 那是 Python 版遗留会话文件的名字，两边的
/// 消息数组字段名不同（Python 是 session，Rust 是 messages），
/// 复用会让旧数据在下次写回时被静默清空。
const UNIFIED_CHANNEL: &str = "feishu_unified";

/// 一条待处理的飞书消息，已从事件 JSON 中提取好
struct IncomingMessage {
    message_id: String,
    chat_id: String,
    chat_type: String,
    sender_open_id: String,
    message_type: String,
    /// 已剥离 @ 占位符的正文
    text: String,
    /// 图片消息的资源 key（message_type == "image" 时存在）
    image_key: Option<String>,
    mentioned_bot: bool,
}

impl IncomingMessage {
    fn is_group(&self) -> bool {
        self.chat_type != "p2p"
    }

    /// 会话 key，直接决定 data/memory/session/current/<key>.json
    fn channel_key(&self, mode: FeishuSessionMode) -> String {
        match mode {
            FeishuSessionMode::Unified => UNIFIED_CHANNEL.to_string(),
            FeishuSessionMode::Isolated => {
                let raw = if self.is_group() {
                    format!("feishu_group_{}", self.chat_id)
                } else {
                    format!("feishu_p2p_{}", self.sender_open_id)
                };
                sanitize_channel(&raw)
            }
        }
    }

    /// 待合并图片的分组 key。与 session_mode 无关，始终按聊天局部化 ——
    /// 统一模式下所有人共享会话，但图片不应跨聊天合并：A 私聊发的图
    /// 不该被 B 在群里的一句文字带走
    fn pending_key(&self) -> String {
        sanitize_channel(&format!("feishu_chat_{}", self.chat_id))
    }

    /// 落盘用的结构化来源信息，对齐 Python 版 _build_meta。
    ///
    /// 只存 open_id 不查通讯录 —— 姓名交给通讯录工具按需维护。
    /// sender_id 是带渠道前缀的规范 id（feishu:ou_xxx），供通讯录工具判断
    /// 「当前是谁」；sender 保留裸 open_id 兼容旧会话。
    ///
    /// 这份 meta 会随每条历史消息进 system prompt，所以恒定值一律不存：
    /// is_bot 恒为 false（parse_message 已过滤非真人消息）；渠道名不单独存 ——
    /// sender_id 的前缀已经标明了来源。
    fn meta(&self) -> Value {
        json!({
            "message_id": self.message_id,
            "chat_id": self.chat_id,
            "chat_type": self.chat_type,
            "sender": self.sender_open_id,
            "sender_id": format!("feishu:{}", self.sender_open_id),
            "content_type": self.message_type,
        })
    }

    /// 仅本轮注入给模型的来源提示，不落盘。
    ///
    /// 历史消息的来源由 meta 随会话进 system prompt 自动可见，但「当前这一条」
    /// 尚未落盘，system prompt 里没有它，所以要在正文前临时挂一行。
    /// 不含 message_id —— 表情回应、消息引用这类工具应由代码从当前消息注入 id，
    /// 让模型转抄 32 位哈希既费 token 又容易出错。
    /// 指定正文的来源提示。图片消息正文为空时，调用方会换成「发来一张图片」
    fn source_hint_with(&self, text: &str) -> String {
        let scene = if self.is_group() {
            "飞书群聊"
        } else {
            "飞书私聊"
        };
        format!("[{scene} | feishu:{}]\n{}", self.sender_open_id, text)
    }
}

pub struct MessageHandler {
    engine: Arc<Engine>,
    api: FeishuApi,
    config: FeishuConfig,
    /// 机器人自身 open_id，用于判断群聊里是否 @ 了自己
    bot_open_id: String,
    /// 配置句柄。只用于 owner 自动填充 —— 其余读配置一律走上面的 config 快照
    store: ConfigStore,
    dedup: Mutex<Dedup>,
    /// 每会话一把串行锁
    slots: SlotRegistry,
    /// 等待用户文字说明的图片，按会话 key 分组
    pending_images: Mutex<PendingImages>,
    /// 表情能力的熔断开关，跨消息共享
    reactions: ReactionGate,
}

impl MessageHandler {
    pub fn new(
        engine: Arc<Engine>,
        api: FeishuApi,
        config: FeishuConfig,
        bot_open_id: String,
        store: ConfigStore,
    ) -> Self {
        Self {
            engine,
            api,
            config,
            bot_open_id,
            store,
            dedup: Mutex::new(Dedup::new(DEDUP_TTL, DEDUP_CAPACITY)),
            slots: SlotRegistry::new(),
            pending_images: Mutex::new(PendingImages::default()),
            reactions: ReactionGate::new(),
        }
    }

    /// 首次私聊时把发信人记为主人，供 feishu_notify_owner 使用。
    ///
    /// 必须在准入判定之后调用 —— 放在白名单过滤之前的话，
    /// 组织里任意一个未授权的人私聊一句就能把自己封为主人。
    ///
    /// 判空刻意走 store.get() 而不是 self.config：后者是渠道启动时的快照，
    /// 永不更新，落盘之后它里面依然是空，会导致每条私聊都重复写一次配置。
    fn record_owner(&self, msg: &IncomingMessage) {
        // 群里 @ 一下不代表这人是主人，只认私聊
        if msg.chat_type != "p2p" || msg.sender_open_id.is_empty() {
            return;
        }
        let mut next = self.store.get();
        if !next.feishu.owner_open_id.is_empty() {
            return;
        }
        next.feishu.owner_open_id = msg.sender_open_id.clone();
        match self.store.save(next) {
            Ok(_) => log::info!(
                target: "feishu",
                "已将 {} 记为主人，feishu_notify_owner 将发给该用户",
                msg.sender_open_id
            ),
            Err(e) => log::warn!(target: "feishu", "写入主人 open_id 失败: {e:#}"),
        }
    }

    /// 取（或新建）某会话的串行锁
    fn slot(&self, channel: &str) -> Arc<crate::channel::common::ChannelSlot> {
        self.slots.slot(channel)
    }

    /// 处理一条原始事件。内部吞掉所有错误，只打日志 —— 单条消息失败不该影响长连接
    pub async fn handle(&self, payload: Vec<u8>) {
        if let Err(e) = self.try_handle(payload).await {
            log::error!(target: "feishu", "处理消息失败: {e:#}");
        }
    }

    async fn try_handle(&self, payload: Vec<u8>) -> Result<()> {
        let event: Value = serde_json::from_slice(&payload)?;

        // 只认消息接收事件，其余（进群、表情回复等）本期不处理
        let event_type = event
            .pointer("/header/event_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type != "im.message.receive_v1" {
            return Ok(());
        }

        let event_id = event
            .pointer("/header/event_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !event_id.is_empty() && !self.dedup.lock().unwrap().accept(event_id) {
            log::debug!(target: "feishu", "重复事件已忽略: {event_id}");
            return Ok(());
        }

        let Some(msg) = self.parse_message(&event) else {
            return Ok(());
        };

        // —— 准入判定 ——

        // 群聊里没被 @ 就当没听见，否则群里每句话都会触发一次 ReAct
        if msg.chat_type != "p2p" && self.config.group_require_mention && !msg.mentioned_bot {
            return Ok(());
        }

        if !self.config.allow_users.is_empty()
            && !self.config.allow_users.contains(&msg.sender_open_id)
        {
            log::info!(target: "feishu", "用户 {} 不在白名单，已忽略", msg.sender_open_id);
            return Ok(());
        }

        match msg.message_type.as_str() {
            "text" => {}
            // 图片资源在准入之后才下载：没资格的人不该触发网络请求
            "image" if msg.image_key.is_some() => {}
            _ => {
                self.api
                    .send_text(&msg.chat_id, "目前只能处理文本和图片消息哦。")
                    .await?;
                return Ok(());
            }
        }

        if msg.text.trim().is_empty() && msg.image_key.is_none() {
            return Ok(());
        }

        // 准入全部通过，这才是一个「有资格成为主人」的人
        self.record_owner(&msg);

        self.dispatch(msg).await
    }

    /// 图片消息先挂起并回一句询问，文字消息到达后再合并处理。
    ///
    /// 外部渠道的图文通常分两条消息发送：收到图片就调用模型，等于在用户
    /// 还没提出问题时先跑一次视觉理解。这里先下载落盘、存进 pending，再发
    /// 一段可配置的询问文本；下一条文字消息会把这些图片作为附件一起送进
    /// engine。询问文本为空时退回旧的「收到即处理」行为。
    async fn dispatch(&self, msg: IncomingMessage) -> Result<()> {
        let ack = self.config.image_ack_text.trim();
        let image_only = msg.image_key.is_some() && msg.text.trim().is_empty();
        if !image_only || ack.is_empty() {
            return self.run_engine(msg).await;
        }

        let Some(image_key) = msg.image_key.clone() else {
            return self.run_engine(msg).await;
        };
        let pending_key = msg.pending_key();

        let path = match self.save_image(&msg, &image_key).await {
            Ok(path) => path,
            Err(e) => {
                log::warn!(target: "feishu", "图片下载失败: {e:#}");
                self.api
                    .send_text(&msg.chat_id, &format!("图片接收失败：{e:#}"))
                    .await?;
                return Ok(());
            }
        };

        // 连发多张图只询问一次；同会话的图片一起等下一段文字
        let ask = self
            .pending_images
            .lock()
            .unwrap()
            .push(pending_key, path, Instant::now());

        if ask {
            if let Err(e) = self.api.send_text(&msg.chat_id, ack).await {
                log::warn!(target: "feishu", "发送图片确认文本失败: {e:#}");
            }
        }
        Ok(())
    }

    /// 取走某会话待处理的图片；过期项直接丢弃
    fn take_pending(&self, channel: &str) -> Vec<ChatAttachment> {
        self.pending_images
            .lock()
            .unwrap()
            .take(channel, Instant::now())
            .into_iter()
            .map(|path| ChatAttachment { path })
            .collect()
    }

    /// 驱动 ReAct 循环。输出分两路：正文走独立消息，思考与工具调用走流式卡片。
    ///
    /// 整个过程用表情标记包住 —— 用户在自己发的那条消息上就能看到状态，
    /// 不必盯着机器人有没有回话，在消息列表里也一眼可见。
    async fn run_engine(&self, msg: IncomingMessage) -> Result<()> {
        let channel = msg.channel_key(self.config.session_mode);
        let cfg = &self.config;
        let mut mark = ReactionTracker::new(&self.api, &self.reactions, &msg.message_id);

        // 同一会话内串行：拿不到锁就排队等，而不是直接拒绝 ——
        // 统一模式下所有人共用一个会话，拒绝会让群里其他人莫名收不到回应。
        // 但也不能无限积压，超过阈值就不接了。
        let slot = self.slot(&channel);
        if slot.waiting.fetch_add(1, Ordering::SeqCst) >= MAX_WAITING {
            slot.waiting.fetch_sub(1, Ordering::SeqCst);
            // 优先只打表情：群里连发时逐条回「排队太多」会把会话刷爆。
            // 表情打不上（没权限）才退回发文本，否则用户就完全没有反馈了
            if !mark.set(&cfg.rejected_reaction).await {
                self.api
                    .send_text(
                        &msg.chat_id,
                        "我这边排队的消息有点多，等我忙完这几条再来吧。",
                    )
                    .await?;
            }
            return Ok(());
        }

        // 抢得到锁说明前面没人，直接进「处理中」；抢不到才先标「排队」，
        // 让用户能区分「轮到我了」和「还在等前面的」
        let _guard = match slot.lock.try_lock() {
            Ok(guard) => {
                mark.set(&cfg.processing_reaction).await;
                guard
            }
            Err(_) => {
                mark.set(&cfg.queued_reaction).await;
                let guard = slot.lock.lock().await;
                mark.set(&cfg.processing_reaction).await;
                guard
            }
        };
        slot.waiting.fetch_sub(1, Ordering::SeqCst);

        // 之前先收到、还在等文字说明的图片，和这条消息一起处理
        let pending = self.take_pending(&msg.pending_key());
        let result = self.drive(&msg, &channel, pending).await;

        // 成败要分开标：之前无论如何都打「完成」，ReAct 崩了也显示 ✅
        let final_emoji = if result.is_ok() {
            &cfg.done_reaction
        } else {
            &cfg.failed_reaction
        };
        mark.set(final_emoji).await;

        result
    }

    /// 下载飞书图片并落进 FileStore，返回 `file_root` 相对路径。
    /// 后续观察/压缩/会话快照都复用桌面端同一条视觉链路
    async fn save_image(&self, msg: &IncomingMessage, image_key: &str) -> Result<String> {
        let bytes = self.api.download_image(&msg.message_id, image_key).await?;
        let stored = self.engine.file_store().save_image(SaveRequest {
            bytes: &bytes,
            original_name: "",
            declared_mime: None,
            source: FileSource::Feishu,
            session_id: None,
            message_id: Some(&msg.message_id),
        })?;
        Ok(stored.path)
    }

    /// 跑一次 ReAct 并把输出推给飞书。
    /// `pending` 是此前已下载、等本条文字一起处理的图片
    async fn drive(
        &self,
        msg: &IncomingMessage,
        channel: &str,
        pending: Vec<ChatAttachment>,
    ) -> Result<()> {
        // 幂等：已有 active 会话就复用，没有才新建
        self.engine.init_session(channel)?;

        let cancel: Arc<AtomicBool> = self.engine.begin_chat(channel);

        // 图片消息没有文字：给模型和会话补一句占位，否则历史里只剩一个附件
        let mut history_text = if msg.text.trim().is_empty() {
            "发来一张图片".to_string()
        } else {
            msg.text.clone()
        };
        let mut attachments = pending;
        if let Some(image_key) = &msg.image_key {
            match self.save_image(msg, image_key).await {
                Ok(path) => attachments.push(ChatAttachment { path }),
                Err(e) => {
                    // 下载/落盘失败不丢整条消息：把原因写进正文，用户知道发生了什么
                    let note = format!("[图片下载失败：{e:#}]");
                    history_text.push_str("\n\n");
                    history_text.push_str(&note);
                }
            }
        }

        // 来源提示只喂给模型，落盘存原文 —— 否则 open_id 会在正文和 meta 里各存一份
        let hinted = msg.source_hint_with(&history_text);
        let input = UserInput {
            for_model: &hinted,
            for_history: &history_text,
            meta: Some(msg.meta()),
            attachments,
        };

        // on_event 是同步闭包，里面不能 await，所以所有推送都挪到 pump 任务里
        let (tx, rx) = mpsc::unbounded_channel::<EngineEvent>();
        let pump =
            tauri::async_runtime::spawn(stream::pump(self.api.clone(), msg.chat_id.clone(), rx));

        let run_result = self
            .engine
            .run_with_input(input, channel, cancel, move |ev| {
                // 只做转发，绝不阻塞 ReAct；接收端提前没了也无所谓
                let _ = tx.send(ev);
            })
            .await;

        // run_with_input 按值持有闭包，返回时把它连同 tx 一起 drop，
        // pump 那边的 recv() 于是返回 None 自然收尾 —— 不需要额外的停止信号
        let stream::PumpResult {
            card,
            mut renderer,
            sent_any,
        } = pump.await.context("卡片推送任务异常退出")?;

        if let Err(e) = &run_result {
            log::error!(target: "feishu", "ReAct 执行失败 channel={channel}: {e:#}");
            renderer.push_error(&format!("处理出错：{e}"));
            // 正文一条都没发出去的话，错误就只躺在折叠面板里没人看得见
            if !sent_any {
                self.api
                    .send_text(&msg.chat_id, &format!("处理出错了：{e}"))
                    .await?;
            }
        } else if !sent_any && renderer.is_empty() {
            self.api
                .send_text(&msg.chat_id, "（这次没有产出内容）")
                .await?;
        }

        // 过程卡片收尾：推最终内容 → 关流式 → 折叠。
        // 纯聊天时卡片压根没建，finish 内部直接返回
        if let Err(e) = card.finish(&renderer.render()).await {
            log::warn!(target: "feishu", "过程卡片收尾失败: {e:#}");
        }
        Ok(())
    }

    /// 从事件 JSON 中提取消息要素
    fn parse_message(&self, event: &Value) -> Option<IncomingMessage> {
        let message = event.pointer("/event/message")?;

        // 只处理真人发言，机器人消息会造成自问自答
        let sender_type = event
            .pointer("/event/sender/sender_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if sender_type != "user" {
            return None;
        }

        let chat_id = message.get("chat_id").and_then(Value::as_str)?.to_string();
        // message_id 供后续的表情回应 / 消息引用工具使用
        let message_id = message
            .get("message_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let chat_type = message
            .get("chat_type")
            .and_then(Value::as_str)
            .unwrap_or("p2p")
            .to_string();
        let message_type = message
            .get("message_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let sender_open_id = event
            .pointer("/event/sender/sender_id/open_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        // content 是「JSON 的字符串形式」，要二次解析
        let raw_content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let content: Value = serde_json::from_str(raw_content).unwrap_or(Value::Null);
        let mut text = content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let image_key = if message_type == "image" {
            content
                .get("image_key")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        } else {
            None
        };

        // mentions[].key 形如 @_user_1，是正文里的占位符，要按 key 精确剔除
        let mut mentioned_bot = false;
        if let Some(mentions) = message.get("mentions").and_then(Value::as_array) {
            for m in mentions {
                let key = m.get("key").and_then(Value::as_str).unwrap_or_default();
                let open_id = m
                    .pointer("/id/open_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                if !self.bot_open_id.is_empty() && open_id == self.bot_open_id {
                    mentioned_bot = true;
                }
                if !key.is_empty() {
                    text = text.replace(key, "");
                }
            }
        }

        Some(IncomingMessage {
            message_id,
            chat_id,
            chat_type,
            sender_open_id,
            message_type,
            text: text.trim().to_string(),
            image_key,
            mentioned_bot,
        })
    }
}

#[cfg(test)]
mod pending_tests {
    use super::*;
    use crate::channel::common::pending::PENDING_IMAGE_TTL;

    #[test]
    fn multiple_images_in_one_channel_ask_once() {
        let mut pending = PendingImages::default();
        let now = Instant::now();

        assert!(
            pending.push("chat-a".into(), "1.png".into(), now),
            "首张应询问"
        );
        assert!(
            !pending.push("chat-a".into(), "2.png".into(), now),
            "连发第二张不再询问"
        );
        assert!(
            pending.push("chat-b".into(), "3.png".into(), now),
            "不同会话各自询问"
        );

        assert_eq!(pending.take("chat-a", now), vec!["1.png", "2.png"]);
        assert!(pending.take("chat-a", now).is_empty(), "取走后应清空");
        assert_eq!(pending.take("chat-b", now), vec!["3.png"]);
    }

    #[test]
    fn expired_images_are_dropped_on_take() {
        let mut pending = PendingImages::default();
        let start = Instant::now();
        pending.push("chat-a".into(), "old.png".into(), start);

        let later = start + PENDING_IMAGE_TTL;
        assert!(pending.take("chat-a", later).is_empty());
    }

    #[test]
    fn push_prunes_expired_entries_of_other_channels() {
        let mut pending = PendingImages::default();
        let start = Instant::now();
        pending.push("chat-a".into(), "old.png".into(), start);

        let later = start + PENDING_IMAGE_TTL;
        assert!(pending.push("chat-b".into(), "new.png".into(), later));
        assert!(pending.take("chat-a", later).is_empty(), "过期项应被清理");
        assert_eq!(pending.take("chat-b", later), vec!["new.png"]);
    }
}
