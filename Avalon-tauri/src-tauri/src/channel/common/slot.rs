// 每会话串行锁
//
// 统一模式下多个聊天共享一个会话文件，并发处理会互相覆盖（丢更新）。隔离模式下
// 不同聊天可以并发，但同一聊天内的消息必须严格串行 —— 否则两个 ReAct 会同时
// 往同一条历史里追加，顺序与内容都不可预期。
//
// 用 per-channel 异步锁达到这个效果，而不是全局 FIFO：隔离模式天然获得并发。
// waiting 用来限制排队长度，避免机器人被刷爆后无限积压。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;

/// 单个会话最多允许多少条消息排队等待。超出直接告知用户
pub const MAX_WAITING: usize = 3;

pub struct ChannelSlot {
    /// 字段刻意 pub(crate)：处理器需要先 try_lock 再决定是否标「排队中」，
    /// 封装成一个方法反而要把状态标记顺序也塞进通用层，职责就混了
    pub(crate) lock: AsyncMutex<()>,
    pub(crate) waiting: std::sync::atomic::AtomicUsize,
}

pub struct SlotRegistry {
    slots: Mutex<HashMap<String, Arc<ChannelSlot>>>,
}

impl SlotRegistry {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// 取（或新建）某会话的串行锁
    pub fn slot(&self, channel: &str) -> Arc<ChannelSlot> {
        self.slots
            .lock()
            .unwrap()
            .entry(channel.to_string())
            .or_insert_with(|| {
                Arc::new(ChannelSlot {
                    lock: AsyncMutex::new(()),
                    waiting: std::sync::atomic::AtomicUsize::new(0),
                })
            })
            .clone()
    }
}

impl Default for SlotRegistry {
    fn default() -> Self {
        Self::new()
    }
}
