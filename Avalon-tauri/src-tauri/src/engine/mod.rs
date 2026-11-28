// Engine 编排层模块
//
// 职责：组装系统提示词、驱动 ReAct 双层循环、发射中间事件、编排工具调用与会话持久化。
// Engine struct 聚合依赖（config/llm/prompt/tools/session），放 Arc<Engine> 进 Tauri State，
// 供 command / scheduler（定时任务）/ channel（飞书、QQ）共享同一句柄（决策 D1）。
// 依赖反转：engine 依赖 tool/session 的 trait，不 import 具体实现。

#![allow(dead_code)]

pub mod events;
pub mod history;
pub mod react;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::channel::contact::ContactStore;
use crate::config::ConfigStore;
use crate::llm::LlmState;
use crate::prompt::PromptAssembler;
use crate::session::{
    ContextUsage, LoadHistoryResult, MessageOrigin, MessageSource, SessionData, SessionMeta,
    SessionStore, TrustLevel,
};
use crate::tool::ToolRegistry;
use crate::usage::UsageStore;
use crate::vector::{RebuildProgress, RebuildStats};
use crate::vision::{ChatAttachment, VisionService};

pub use events::EngineEvent;

/// 一次用户输入。
///
/// 「发给模型的」与「落盘保存的」刻意分开：渠道消息需要让模型当场知道来源
/// （谁、在哪儿说的），但这些信息已经结构化存在 origin 里，随会话序列化进
/// system prompt 供后续轮次读取。
pub struct UserInput<'a> {
    /// 发给模型的正文，可含仅本轮有效的来源提示
    pub for_model: &'a str,
    /// 落盘保存的原文
    pub for_history: &'a str,
    /// 消息来源；桌面端为 None，由 engine 补默认 origin
    pub meta: Option<MessageOrigin>,
    /// 本轮图片附件（只含后端相对路径）。由 VisionService 转成图片字节或观察文本
    pub attachments: Vec<ChatAttachment>,
}

impl<'a> UserInput<'a> {
    /// 桌面端等无渠道来源的输入：模型看到的与落盘的一致
    pub fn plain(text: &'a str) -> Self {
        Self {
            for_model: text,
            for_history: text,
            meta: None,
            attachments: Vec::new(),
        }
    }
}

/// 本次 run 的回复目标。仅由触发方持有，engine 不决定消息发往哪个渠道。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "channel", rename_all = "lowercase")]
pub enum ReplyTarget {
    Desktop,
    Feishu {
        chat_id: String,
        message_id: Option<String>,
    },
    Qq {
        chat_id: String,
        msg_idx: Option<String>,
    },
    /// 定时任务等不自动回复；需要通知时由任务配置主动投递
    None,
}

/// 一次 ReAct 执行的上下文（不是会话身份）。
#[derive(Debug, Clone, Serialize)]
pub struct RunContext {
    pub run_id: String,
    pub origin: MessageOrigin,
    pub reply: ReplyTarget,
}

impl RunContext {
    pub fn new(run_id: impl Into<String>, origin: MessageOrigin, reply: ReplyTarget) -> Self {
        Self {
            run_id: run_id.into(),
            origin,
            reply,
        }
    }

    pub fn desktop(run_id: impl Into<String>) -> Self {
        Self::new(run_id, MessageOrigin::desktop_owner(), ReplyTarget::Desktop)
    }

    /// 队列优先级：桌面 > 主人私聊 > 群聊 > 定时任务
    pub fn priority(&self) -> u8 {
        match self.origin.source {
            MessageSource::Desktop => 0,
            MessageSource::Task => 3,
            _ => {
                let group = self
                    .origin
                    .chat_type
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case("group"))
                    .unwrap_or(false);
                if group || self.origin.trust != TrustLevel::Owner {
                    2
                } else {
                    1
                }
            }
        }
    }
}

/// 广播给前端的事件包装：桌面统一时间线按 run_id 归组，并区分来源。
#[derive(Debug, Clone, Serialize)]
pub struct SessionEvent {
    pub run_id: String,
    pub origin: MessageOrigin,
    pub event: EngineEvent,
}

/// 全局单写者队列。
///
/// 所有 run（桌面 / 飞书 / QQ / 定时任务）排队串行执行；同优先级 FIFO，
/// 数值越小优先级越高。控制操作通过 `pending_count()` 判断是否可以执行。
struct RunQueue {
    inner: Mutex<QueueInner>,
    notify: tokio::sync::Notify,
}

struct QueueInner {
    next_ticket: u64,
    running: bool,
    waiting: BTreeMap<(u8, u64), ()>,
}

struct RunPermit<'a> {
    queue: &'a RunQueue,
}

impl Drop for RunPermit<'_> {
    fn drop(&mut self) {
        let mut inner = self.queue.inner.lock().unwrap();
        inner.running = false;
        drop(inner);
        // 必须唤醒全部等待者：只唤醒一个的话，被唤醒者未必是新的队首
        // （高优先级后插入），它会重新睡下，队列此后无人唤醒，全局卡死。
        self.queue.notify.notify_waiters();
    }
}

/// 排队 ticket 的守卫：等待中的 future 被 drop（任务 abort / select 取消）时，
/// 从等待队列移除自己，否则幽灵 ticket 会永久占据队首。
struct WaitTicket<'a> {
    queue: &'a RunQueue,
    key: Option<(u8, u64)>,
}

impl Drop for WaitTicket<'_> {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else { return };
        let mut inner = self.queue.inner.lock().unwrap();
        let removed = inner.waiting.remove(&key).is_some();
        drop(inner);
        if removed {
            self.queue.notify.notify_waiters();
        }
    }
}

impl RunQueue {
    fn new() -> Self {
        Self {
            inner: Mutex::new(QueueInner {
                next_ticket: 0,
                running: false,
                waiting: BTreeMap::new(),
            }),
            notify: tokio::sync::Notify::new(),
        }
    }

    async fn acquire(&self, priority: u8) -> RunPermit<'_> {
        let key = {
            let mut inner = self.inner.lock().unwrap();
            let key = (priority, inner.next_ticket);
            inner.next_ticket += 1;
            inner.waiting.insert(key, ());
            key
        };

        let mut ticket = WaitTicket {
            queue: self,
            key: Some(key),
        };

        loop {
            let notified = self.notify.notified();
            let mut notified = std::pin::pin!(notified);
            // 先注册等待者再检查状态：notify_waiters 不保存许可，检查后才注册会丢唤醒
            let _ = notified.as_mut().enable();
            {
                let mut inner = self.inner.lock().unwrap();
                let first = inner.waiting.keys().next().copied();
                if !inner.running && first == Some(key) {
                    inner.waiting.remove(&key);
                    inner.running = true;
                    ticket.key = None; // 正常获得执行权：守卫退出时不再摘 ticket
                    return RunPermit { queue: self };
                }
            }
            notified.await;
        }
    }

    /// 正在执行 + 排队中的数量
    fn pending_count(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.waiting.len() + usize::from(inner.running)
    }

    fn is_running(&self) -> bool {
        self.inner.lock().unwrap().running
    }
}

/// ReAct 编排引擎（字段全为共享/clone 类型，engine 本身无状态）
pub struct Engine {
    config: ConfigStore,
    llm: LlmState,
    prompt: Arc<PromptAssembler>,
    tools: Arc<dyn ToolRegistry>,
    session: Arc<dyn SessionStore>,
    usage: Arc<UsageStore>,
    vision: Arc<VisionService>,
    /// 渠道通讯录。工具上下文靠它判断当前发言人是否主人
    contacts: Arc<ContactStore>,
    /// 全局单写者队列
    queue: RunQueue,
    /// 按 run_id 维度的取消标志（run 提交时注册，stop_chat 置位中断）
    cancels: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
    /// 正在执行 / 排队中的 run 来源，供桌面展示
    active: Mutex<BTreeMap<String, RunContext>>,
    /// Tauri AppHandle；setup 阶段注入，用于广播 session-event
    app: Mutex<Option<AppHandle>>,
}

impl Engine {
    #[allow(clippy::too_many_arguments)] // 装配入口，依赖按类型注入，打包无收益
    pub fn new(
        config: ConfigStore,
        llm: LlmState,
        prompt: Arc<PromptAssembler>,
        tools: Arc<dyn ToolRegistry>,
        session: Arc<dyn SessionStore>,
        usage: Arc<UsageStore>,
        vision: Arc<VisionService>,
        contacts: Arc<ContactStore>,
    ) -> Self {
        Self {
            config,
            llm,
            prompt,
            tools,
            session,
            usage,
            vision,
            contacts,
            queue: RunQueue::new(),
            cancels: Mutex::new(BTreeMap::new()),
            active: Mutex::new(BTreeMap::new()),
            app: Mutex::new(None),
        }
    }

    /// setup 阶段注入 AppHandle，之后每次 run 的事件都会广播到前端。
    pub fn attach_app(&self, app: AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }

    /// 文件存储句柄。渠道层下载图片后需要落盘，复用视觉服务持有的同一个 FileStore
    pub(crate) fn file_store(&self) -> &crate::file::FileStore {
        self.vision.file_store()
    }

    /// 生成一个全局唯一 run_id（时间戳 + 进程内自增，避免同一毫秒冲突）
    pub fn new_run_id(prefix: &str) -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!(
            "{prefix}_{}_{n}",
            chrono::Local::now().format("%Y%m%d%H%M%S%3f")
        )
    }

    /// 跑一次完整 ReAct 循环，进入全局单写者队列，事件同时广播到前端。
    pub async fn run_with_input(
        &self,
        input: UserInput<'_>,
        run: RunContext,
        on_event: impl FnMut(EngineEvent) + Send,
    ) -> Result<()> {
        let run_id = run.run_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels
            .lock()
            .unwrap()
            .insert(run_id.clone(), cancel.clone());
        self.active
            .lock()
            .unwrap()
            .insert(run_id.clone(), run.clone());

        let result = self
            .execute(input, &run, cancel, on_event)
            .await;

        self.cancels.lock().unwrap().remove(&run_id);
        self.active.lock().unwrap().remove(&run_id);
        self.emit("session-updated", serde_json::json!({ "run_id": run_id }));
        result
    }

    async fn execute(
        &self,
        input: UserInput<'_>,
        run: &RunContext,
        cancel: Arc<AtomicBool>,
        on_event: impl FnMut(EngineEvent) + Send,
    ) -> Result<()> {
        let _permit = self.queue.acquire(run.priority()).await;

        let mut on_event = on_event;
        let app = self.app.lock().unwrap().clone();
        let run_id = run.run_id.clone();
        let origin = run.origin.clone();
        let mut emit = move |event: EngineEvent| {
            if let Some(app) = &app {
                let payload = SessionEvent {
                    run_id: run_id.clone(),
                    origin: origin.clone(),
                    event: event.clone(),
                };
                if let Err(e) = app.emit("session-event", payload) {
                    log::warn!(target: "engine", "广播 session-event 失败: {e}");
                }
            }
            on_event(event);
        };

        react::run_loop(
            input,
            run,
            &self.config,
            &self.llm,
            self.prompt.as_ref(),
            self.tools.as_ref(),
            self.session.as_ref(),
            self.usage.as_ref(),
            self.vision.as_ref(),
            self.contacts.as_ref(),
            &cancel,
            &mut emit,
        )
        .await
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            if let Err(e) = app.emit(event, payload) {
                log::warn!(target: "engine", "广播 {event} 失败: {e}");
            }
        }
    }

    /// 置位某 run 的取消标志；run 收尾时整轮丢弃，不落盘。
    pub fn stop_chat(&self, run_id: &str) {
        if let Some(flag) = self.cancels.lock().unwrap().get(run_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// 停止所有执行中 / 排队中的 run（退出应用等场景）
    pub fn stop_all(&self) {
        for flag in self.cancels.lock().unwrap().values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// 正在执行 + 排队中的数量。控制操作（新建/切换/删除）在 > 0 时拒绝。
    pub fn pending_count(&self) -> usize {
        self.queue.pending_count()
    }

    pub fn is_busy(&self) -> bool {
        self.queue.is_running()
    }

    /// 当前执行中 / 排队中的 run 来源快照（前端显示「正在处理来自飞书的请求」）。
    pub fn active_runs(&self) -> Vec<RunContext> {
        self.active.lock().unwrap().values().cloned().collect()
    }

    /// 初始化会话（active 复用 / 否则新建）—— 调用方在 run 前触发（决策 D3）
    pub fn init_session(&self) -> Result<()> {
        self.session.init_session()
    }

    /// 设置当前活跃会话标题（仅手动改名使用）
    pub fn set_current_title(&self, title: &str) -> Result<()> {
        self.session.set_current_title(title)
    }

    /// 新建会话：归档当前（若非空）+ 创建新 active 会话。
    /// 控制操作进入同一队列，优先级高于普通 run。
    pub async fn create_session(&self) -> Result<SessionData> {
        let _permit = self.queue.acquire(0).await;
        self.session.create_session().await
    }

    /// 读取当前会话完整数据（供前端加载历史消息）
    pub fn get_current_session(&self) -> Result<SessionData> {
        self.session.get_current_session()
    }

    /// 读取当前会话上下文用量（供前端圆形进度条展示）
    pub fn get_context_usage(&self) -> Result<ContextUsage> {
        self.session.get_context_usage()
    }

    /// 归档当前会话（压缩 + 移入 history）
    pub async fn save_session(&self) -> Result<()> {
        let _permit = self.queue.acquire(0).await;
        self.session.save_current_session().await
    }

    /// 主动压缩当前会话（不归档），返回是否实际压缩
    pub async fn compress_session(&self) -> Result<bool> {
        let _permit = self.queue.acquire(0).await;
        self.session.compress_session().await
    }

    /// 重建会话向量库（维护操作，设置页触发）
    pub fn rebuild_memory_index(
        &self,
        on_progress: impl Fn(RebuildProgress) + Send + Sync,
    ) -> Result<RebuildStats> {
        self.session.rebuild_index(&on_progress)
    }

    /// 列出全部会话元信息（active 置顶 + 归档按时间倒序）
    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>> {
        self.session.list_sessions()
    }

    /// 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据
    pub async fn switch_session(&self, id: &str) -> Result<SessionData> {
        let _permit = self.queue.acquire(0).await;
        self.session.switch_session(id).await
    }

    /// 渐进式加载历史块
    pub fn load_session_history(
        &self,
        id: &str,
        before_chunk: Option<u64>,
    ) -> Result<LoadHistoryResult> {
        self.session.load_session_history(id, before_chunk)
    }

    /// 删除归档会话（目录 + 向量库 chunk）
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.session.delete_session(id)
    }

    /// 重命名会话标题
    pub fn rename_session(&self, id: &str, title: &str) -> Result<()> {
        self.session.rename_session(id, title)
    }
}
