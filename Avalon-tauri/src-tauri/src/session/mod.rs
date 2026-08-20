// 会话管理模块
//
// 职责：会话文件生命周期（初始化/追加/归档）+ 自动压缩 + 渐进式总结（永恒会话）
// + 限界上下文 + 向量索引重建编排。
// 定义 SessionStore 契约（依赖反转：engine 依赖本模块的 trait），FileSessionStore 为落地实现。
// 依赖 llm（压缩）+ prompt（组装压缩提示词）+ vector（摘要入库）+ config（路径/阈值）。

#![allow(dead_code)] // session 模块供未来 engine/tool 引用，当前无调用方，接入后移除
#![allow(unused_imports)] // FileSessionStore 等 re-export 供外部模块引用，mod 内暂未使用

pub mod store;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

use crate::vector::{RebuildProgress, RebuildStats};

pub use store::FileSessionStore;
pub use types::*;

/// 会话存储契约：engine 通过此 trait 操作会话（不依赖具体实现）
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// 初始化：active 则复用，否则新建空会话（id = {channel}_{timestamp}）
    fn init_session(&self, channel: &str) -> Result<()>;
    /// 读取当前会话完整数据（供前端加载历史消息 / 判断会话状态）
    fn get_current_session(&self, channel: &str) -> Result<SessionData>;
    /// 当前会话上下文用量（最大输入 token vs 压缩阈值），供前端圆形进度条展示
    fn get_context_usage(&self, channel: &str) -> Result<ContextUsage>;
    /// 限界会话上下文（JSON 字符串），供 engine 拼进 system_prompt
    fn get_context_for_prompt(&self, channel: &str) -> Result<String>;
    /// 持久化本轮 chat_history（追加到 messages 字段）
    fn update_current_session(&self, channel: &str, chat_history: &[Message]) -> Result<()>;
    /// 自动压缩检查：输入 token 超阈值触发压缩，返回是否触发
    async fn auto_compress_check(&self, channel: &str, chat_history: &[Message]) -> Result<bool>;
    /// 归档当前会话（先压缩，写 history/{id}/index.json，重置 current）
    async fn save_current_session(&self, channel: &str) -> Result<()>;
    /// 重建向量索引：清空 + 重扫 history/current + 重新入库（维护操作，设置页触发）
    /// 逐 session 处理时经 on_progress 上报进度（跨 spawn_blocking 线程，回调须 Send + Sync）
    fn rebuild_index(
        &self,
        on_progress: &(dyn Fn(RebuildProgress) + Send + Sync),
    ) -> Result<RebuildStats>;
}

/// 会话 ID 时间戳：YYYY-MM-DD-HH_MM_SS（下划线，文件名安全，字典序 = 时间序）
pub fn now_id_ts() -> String {
    chrono::Local::now().format("%Y-%m-%d-%H_%M_%S").to_string()
}
