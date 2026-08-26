// 飞书消息事件处理
//
// 从 ws.rs 收到的原始事件 JSON 出发：解析 → 去重 → 准入判定 → 打「处理中」表情 →
// 驱动 Engine::run（正文走消息、过程走卡片，见 stream.rs）→ 打「完成」表情。
//
// 每条消息独立 spawn，彼此不阻塞；同一会话内的串行由本文件的 ChannelSlot 锁保证。
// 之所以要自己去重：ws.rs 是「收到就 ACK」，但网络抖动、服务端重投都可能让同一个
// event_id 到达两次，没有去重就会把同一句话跑两遍 ReAct。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;

use super::api::FeishuApi;
use super::stream;
use crate::config::{FeishuConfig, FeishuSessionMode};
use crate::engine::{Engine, EngineEvent, UserInput};

/// 已处理 event_id 的保留时长
const DEDUP_TTL: Duration = Duration::from_secs(600);
/// 去重表容量上限，超出后清理最旧的一半，防止长期运行内存无限增长
const DEDUP_CAPACITY: usize = 1024;
/// 单个会话最多允许多少条消息排队等待。超出直接告知用户，避免无限积压
const MAX_WAITING: usize = 3;

/// 统一模式下所有飞书消息共用的会话名。
/// 刻意避开裸 "feishu" —— 那是 Python 版遗留会话文件的名字，两边的
/// 消息数组字段名不同（Python 是 session，Rust 是 messages），
/// 复用会让旧数据在下次写回时被静默清空。
const UNIFIED_CHANNEL: &str = "feishu_unified";

/// 每个会话一把异步锁，保证同一会话内的消息严格串行。
///
/// 统一模式下所有聊天共享一个会话文件，并发处理会互相覆盖（丢更新）——
/// Python 版为此用了全局 FIFO 队列。这里用 per-channel 锁达到同样效果，
/// 且隔离模式下不同聊天仍可并发，不需要两套代码。
struct ChannelSlot {
    lock: AsyncMutex<()>,
    waiting: AtomicUsize,
}

/// event_id 去重表
struct Dedup {
    seen: HashMap<String, Instant>,
}

impl Dedup {
    fn new() -> Self {
        Self {
            seen: HashMap::new(),
        }
    }

    /// 首次出现返回 true；重复返回 false
    fn accept(&mut self, id: &str) -> bool {
        let now = Instant::now();
        self.seen.retain(|_, t| now.duration_since(*t) < DEDUP_TTL);

        if self.seen.len() >= DEDUP_CAPACITY {
            // TTL 没来得及清理时的兜底：按时间排序砍掉较旧的一半
            let mut entries: Vec<_> = self.seen.iter().map(|(k, v)| (k.clone(), *v)).collect();
            entries.sort_by_key(|(_, t)| *t);
            for (k, _) in entries.into_iter().take(DEDUP_CAPACITY / 2) {
                self.seen.remove(&k);
            }
        }

        self.seen.insert(id.to_string(), now).is_none()
    }
}

/// 一条待处理的飞书消息，已从事件 JSON 中提取好
struct IncomingMessage {
    message_id: String,
    chat_id: String,
    chat_type: String,
    sender_open_id: String,
    message_type: String,
    /// 已剥离 @ 占位符的正文
    text: String,
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

    /// 落盘用的结构化来源信息，对齐 Python 版 _build_meta。
    ///
    /// 只存 open_id 不查通讯录 —— 姓名交给后续的飞书工具按需查询。
    ///
    /// 这份 meta 会随每条历史消息进 system prompt，所以恒定值一律不存：
    /// is_bot 恒为 false（parse_message 已过滤非真人消息）；渠道名也不存 ——
    /// id 的 oc_ / om_ / ou_ 前缀本身就标明了来源，真接入第二个渠道时
    /// 那边自有自己的 meta 结构，不靠这个字段区分。
    fn meta(&self) -> Value {
        json!({
            "message_id": self.message_id,
            "chat_id": self.chat_id,
            "chat_type": self.chat_type,
            "sender": self.sender_open_id,
            "content_type": self.message_type,
        })
    }

    /// 仅本轮注入给模型的来源提示，不落盘。
    ///
    /// 历史消息的来源由 meta 随会话进 system prompt 自动可见，但「当前这一条」
    /// 尚未落盘，system prompt 里没有它，所以要在正文前临时挂一行。
    /// 不含 message_id —— 表情回应、消息引用这类工具应由代码从当前消息注入 id，
    /// 让模型转抄 32 位哈希既费 token 又容易出错。
    fn source_hint(&self) -> String {
        let scene = if self.is_group() {
            "飞书群聊"
        } else {
            "飞书私聊"
        };
        format!("[{scene} | {}]\n{}", self.sender_open_id, self.text)
    }
}

/// channel 名会当作文件名使用，只保留安全字符
fn sanitize_channel(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub struct MessageHandler {
    engine: Arc<Engine>,
    api: FeishuApi,
    config: FeishuConfig,
    /// 机器人自身 open_id，用于判断群聊里是否 @ 了自己
    bot_open_id: String,
    dedup: Mutex<Dedup>,
    /// 每会话一把串行锁
    slots: Mutex<HashMap<String, Arc<ChannelSlot>>>,
}

impl MessageHandler {
    pub fn new(
        engine: Arc<Engine>,
        api: FeishuApi,
        config: FeishuConfig,
        bot_open_id: String,
    ) -> Self {
        Self {
            engine,
            api,
            config,
            bot_open_id,
            dedup: Mutex::new(Dedup::new()),
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// 取（或新建）某会话的串行锁
    fn slot(&self, channel: &str) -> Arc<ChannelSlot> {
        self.slots
            .lock()
            .unwrap()
            .entry(channel.to_string())
            .or_insert_with(|| {
                Arc::new(ChannelSlot {
                    lock: AsyncMutex::new(()),
                    waiting: AtomicUsize::new(0),
                })
            })
            .clone()
    }

    /// 处理一条原始事件。内部吞掉所有错误，只打日志 —— 单条消息失败不该影响长连接
    pub async fn handle(&self, payload: Vec<u8>) {
        if let Err(e) = self.try_handle(payload).await {
            eprintln!("[飞书] 处理消息失败: {e:#}");
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
            println!("[飞书] 重复事件已忽略: {event_id}");
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
            println!("[飞书] 用户 {} 不在白名单，已忽略", msg.sender_open_id);
            return Ok(());
        }

        if msg.message_type != "text" {
            self.api
                .send_text(&msg.chat_id, "目前只能处理文本消息哦。")
                .await?;
            return Ok(());
        }

        if msg.text.trim().is_empty() {
            return Ok(());
        }

        self.run_engine(msg).await
    }

    /// 驱动 ReAct 循环。输出分两路：正文走独立消息，思考与工具调用走流式卡片。
    ///
    /// 整个过程用表情标记包住 —— 用户在自己发的那条消息上就能看到状态，
    /// 不必盯着机器人有没有回话。
    async fn run_engine(&self, msg: IncomingMessage) -> Result<()> {
        let channel = msg.channel_key(self.config.session_mode);

        // 同一会话内串行：拿不到锁就排队等，而不是直接拒绝 ——
        // 统一模式下所有人共用一个会话，拒绝会让群里其他人莫名收不到回应。
        // 但也不能无限积压，超过阈值就明确告知。
        let slot = self.slot(&channel);
        if slot.waiting.fetch_add(1, Ordering::SeqCst) >= MAX_WAITING {
            slot.waiting.fetch_sub(1, Ordering::SeqCst);
            self.api
                .send_text(&msg.chat_id, "我这边排队的消息有点多，等我忙完这几条再来吧。")
                .await?;
            return Ok(());
        }

        // 抢锁前就打上「处理中」：排队等待时用户也能看到已被接收，
        // 而不是几十秒的杳无音信
        let processing = self.mark_processing(&msg.message_id).await;

        let _guard = slot.lock.lock().await;
        slot.waiting.fetch_sub(1, Ordering::SeqCst);

        let result = self.drive(&msg, &channel).await;

        self.mark_done(&msg.message_id, processing).await;
        result
    }

    /// 跑一次 ReAct 并把输出推给飞书
    async fn drive(&self, msg: &IncomingMessage, channel: &str) -> Result<()> {
        // 幂等：已有 active 会话就复用，没有才新建
        self.engine.init_session(channel)?;

        let cancel: Arc<AtomicBool> = self.engine.begin_chat(channel);

        // 来源提示只喂给模型，落盘存原文 —— 否则 open_id 会在正文和 meta 里各存一份
        let hinted = msg.source_hint();
        let input = UserInput {
            for_model: &hinted,
            for_history: &msg.text,
            meta: Some(msg.meta()),
        };

        // on_event 是同步闭包，里面不能 await，所以所有推送都挪到 pump 任务里
        let (tx, rx) = mpsc::unbounded_channel::<EngineEvent>();
        let pump = tauri::async_runtime::spawn(stream::pump(
            self.api.clone(),
            msg.chat_id.clone(),
            rx,
        ));

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
            eprintln!("[飞书] ReAct 执行失败 channel={channel}: {e:#}");
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
            eprintln!("[飞书] 过程卡片收尾失败: {e:#}");
        }
        Ok(())
    }

    /// 打「处理中」表情，返回 reaction_id 供之后撤销。
    /// 失败只记日志 —— 表情是锦上添花，不该让整条消息处理不下去
    async fn mark_processing(&self, message_id: &str) -> Option<String> {
        let emoji = self.config.processing_reaction.trim();
        if emoji.is_empty() || message_id.is_empty() {
            return None;
        }
        match self.api.add_reaction(message_id, emoji).await {
            Ok(id) if !id.is_empty() => Some(id),
            Ok(_) => None,
            Err(e) => {
                eprintln!("[飞书] 添加处理中表情失败: {e:#}");
                None
            }
        }
    }

    /// 撤掉「处理中」，换上「完成」
    async fn mark_done(&self, message_id: &str, processing: Option<String>) {
        if let Some(id) = processing {
            if let Err(e) = self.api.remove_reaction(message_id, &id).await {
                eprintln!("[飞书] 取消处理中表情失败: {e:#}");
            }
        }
        let emoji = self.config.done_reaction.trim();
        if emoji.is_empty() || message_id.is_empty() {
            return;
        }
        if let Err(e) = self.api.add_reaction(message_id, emoji).await {
            eprintln!("[飞书] 添加完成表情失败: {e:#}");
        }
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
        let raw_content = message.get("content").and_then(Value::as_str).unwrap_or("{}");
        let content: Value = serde_json::from_str(raw_content).unwrap_or(Value::Null);
        let mut text = content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

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
            mentioned_bot,
        })
    }
}
