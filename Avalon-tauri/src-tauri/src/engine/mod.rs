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

use std::sync::Arc;

use anyhow::Result;

use crate::config::ConfigStore;
use crate::llm::LlmState;
use crate::prompt::PromptAssembler;
use crate::session::{SessionData, SessionStore};
use crate::tool::ToolRegistry;
use crate::usage::UsageStore;
use crate::vector::RebuildStats;

pub use events::EngineEvent;

/// ReAct 编排引擎（字段全为共享/clone 类型，engine 本身无状态，天然支持多路并发）
pub struct Engine {
    config: ConfigStore,
    llm: LlmState,
    prompt: PromptAssembler,
    tools: Arc<dyn ToolRegistry>,
    session: Arc<dyn SessionStore>,
    usage: Arc<UsageStore>,
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
        }
    }

    /// 跑一次完整 ReAct 循环：输入用户消息，经事件回调推送中间态，结束返回。
    /// init/save（会话生命周期节点）由调用方触发，update/compress 在循环内自动完成（决策 D3）。
    pub async fn run(
        &self,
        user_input: &str,
        channel: &str,
        on_event: impl FnMut(EngineEvent) + Send,
    ) -> Result<()> {
        let mut on_event = on_event;
        react::run_loop(
            user_input,
            channel,
            &self.config,
            &self.llm,
            &self.prompt,
            self.tools.as_ref(),
            self.session.as_ref(),
            self.usage.as_ref(),
            &mut on_event,
        )
        .await
    }

    /// 初始化会话（channel 维度：active 复用 / 否则新建）—— 调用方在 run 前触发（决策 D3）
    pub fn init_session(&self, channel: &str) -> Result<()> {
        self.session.init_session(channel)
    }

    /// 读取当前会话完整数据（供前端加载历史消息）
    pub fn get_current_session(&self, channel: &str) -> Result<SessionData> {
        self.session.get_current_session(channel)
    }

    /// 归档当前会话（压缩 + 移入 history）—— 调用方在 run 后触发（决策 D3）
    pub async fn save_session(&self, channel: &str) -> Result<()> {
        self.session.save_current_session(channel).await
    }

    /// 重建会话向量库（维护操作，设置页触发）：清空 + 重扫 history/current + 重新入库
    pub fn rebuild_memory_index(&self) -> Result<RebuildStats> {
        self.session.rebuild_index()
    }
}
