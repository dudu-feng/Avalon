// Engine 编排层模块
//
// 职责：组装系统提示词、驱动 ReAct 双层循环、发射中间事件、编排工具调用与会话持久化。
// Engine struct 聚合依赖（config/llm/prompt/tools/session），放 Arc<Engine> 进 Tauri State，
// 供 command / 未来 scheduler（定时任务）/ dream（梦境机制）共享同一句柄（决策 D1）。
// 依赖反转：engine 依赖 tool/session 的 trait，不 import 具体实现。

#![allow(dead_code)]

pub mod events;
pub mod history;
pub mod react;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::config::ConfigStore;
use crate::llm::LlmState;
use crate::prompt::PromptAssembler;
use crate::session::{ContextUsage, LoadHistoryResult, SessionData, SessionMeta, SessionStore};
use crate::tool::ToolRegistry;
use crate::usage::UsageStore;
use crate::vector::{RebuildProgress, RebuildStats};

pub use events::EngineEvent;

/// ReAct 编排引擎（字段全为共享/clone 类型，engine 本身无状态，天然支持多路并发）
pub struct Engine {
    config: ConfigStore,
    llm: LlmState,
    prompt: PromptAssembler,
    tools: Arc<dyn ToolRegistry>,
    session: Arc<dyn SessionStore>,
    usage: Arc<UsageStore>,
    /// 按 channel 维度的取消标志（chat 运行时注册，stop_chat 置位中断流式循环）
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// 正在运行的 channel 集合（心跳调度前判忙，避免并发执行同一任务）
    busy: Mutex<HashSet<String>>,
}

impl Engine {
    pub fn new(
        config: ConfigStore,
        llm: LlmState,
        prompt: PromptAssembler,
        tools: Arc<dyn ToolRegistry>,
        session: Arc<dyn SessionStore>,
        usage: Arc<UsageStore>,
    ) -> Self {
        Self {
            config,
            llm,
            prompt,
            tools,
            session,
            usage,
            cancels: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashSet::new()),
        }
    }

    /// 跑一次完整 ReAct 循环：输入用户消息，经事件回调推送中间态，结束返回。
    /// init/save（会话生命周期节点）由调用方触发，update/compress 在循环内自动完成（决策 D3）。
    pub async fn run(
        &self,
        user_input: &str,
        channel: &str,
        cancel: Arc<AtomicBool>,
        on_event: impl FnMut(EngineEvent) + Send,
    ) -> Result<()> {
        self.busy.lock().unwrap().insert(channel.to_string());
        let mut on_event = on_event;
        let result = react::run_loop(
            user_input,
            channel,
            &self.config,
            &self.llm,
            &self.prompt,
            self.tools.as_ref(),
            self.session.as_ref(),
            self.usage.as_ref(),
            &cancel,
            &mut on_event,
        )
        .await;
        self.busy.lock().unwrap().remove(channel);
        result
    }

    /// 注册一次 chat 运行的取消标志（返回 Arc 供 run 传入），channel 维度的最新一次覆盖旧值
    pub fn begin_chat(&self, channel: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancels
            .lock()
            .unwrap()
            .insert(channel.to_string(), flag.clone());
        flag
    }

    /// 置位某 channel 的取消标志，中断正在进行的流式生成
    pub fn stop_chat(&self, channel: &str) {
        if let Some(flag) = self.cancels.lock().unwrap().get(channel) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// 判断某 channel 是否有正在进行的 ReAct 循环（心跳调度前判忙，避免并发执行同一任务）
    pub fn is_channel_busy(&self, channel: &str) -> bool {
        self.busy.lock().unwrap().contains(channel)
    }

    /// 初始化会话（channel 维度：active 复用 / 否则新建）—— 调用方在 run 前触发（决策 D3）
    pub fn init_session(&self, channel: &str) -> Result<()> {
        self.session.init_session(channel)
    }

    /// 设置当前活跃会话标题（定时任务执行前调用，让会话文件可读：title = 定时任务-任务名称）
    pub fn set_current_title(&self, channel: &str, title: &str) -> Result<()> {
        self.session.set_current_title(channel, title)
    }

    /// 新建会话：归档当前（若非空）+ 创建新 active 会话（写 current + history 初始存档）
    pub async fn create_session(&self, channel: &str) -> Result<SessionData> {
        self.session.create_session(channel).await
    }

    /// 读取当前会话完整数据（供前端加载历史消息）
    pub fn get_current_session(&self, channel: &str) -> Result<SessionData> {
        self.session.get_current_session(channel)
    }

    /// 读取当前会话上下文用量（供前端圆形进度条展示）
    pub fn get_context_usage(&self, channel: &str) -> Result<ContextUsage> {
        self.session.get_context_usage(channel)
    }

    /// 归档当前会话（压缩 + 移入 history）—— 调用方在 run 后触发（决策 D3）
    pub async fn save_session(&self, channel: &str) -> Result<()> {
        self.session.save_current_session(channel).await
    }

    /// 重建会话向量库（维护操作，设置页触发）：清空 + 重扫 history/current + 重新入库
    /// 逐 session 处理时经 on_progress 上报进度（跨 spawn_blocking 线程，回调须 Send + Sync）
    pub fn rebuild_memory_index(
        &self,
        on_progress: impl Fn(RebuildProgress) + Send + Sync,
    ) -> Result<RebuildStats> {
        self.session.rebuild_index(&on_progress)
    }

    /// 列出全部会话元信息（active 置顶 + 归档按时间倒序）
    pub fn list_sessions(&self, channel: &str) -> Result<Vec<SessionMeta>> {
        self.session.list_sessions(channel)
    }

    /// 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据
    pub async fn switch_session(&self, channel: &str, id: &str) -> Result<SessionData> {
        self.session.switch_session(channel, id).await
    }

    /// 渐进式加载历史块：before_chunk=None 读最新块，否则读更早一块（供前端渐进式回溯）
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
    pub fn rename_session(&self, channel: &str, id: &str, title: &str) -> Result<()> {
        self.session.rename_session(channel, id, title)
    }
}
