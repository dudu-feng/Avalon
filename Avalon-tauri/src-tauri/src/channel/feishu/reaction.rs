// 消息进度的表情标记
//
// 在用户发的那条消息上挂一个表情表示当前进度，同一时刻只挂一个 ——
// 切换即「撤旧 + 打新」。它的独特价值是在消息列表里不点进去就能看到状态，
// 所以定位是「状态摘要」而非进度条：思考、工具调用这些过程细节由流式卡片
// 承担（见 stream.rs），表情只标注四个关键节点。
//
// 表情属于锦上添花，任何一步失败都只记日志、不影响对话。但连续失败多半是
// 没开 im:message.reaction:write 权限，此时必须熔断 —— 否则每条消息都要
// 白白试上三次，日志会被刷满。

use std::sync::atomic::{AtomicU32, Ordering};

use super::api::FeishuApi;

/// 连续失败多少次后认定表情能力不可用
const MAX_FAILURES: u32 = 3;

/// 表情能力的熔断开关，跨消息共享。
///
/// 之所以不是「一次失败就永久禁用」：网络抖动导致的偶发失败不该让整个功能
/// 停摆。计数在成功时清零，所以只有持续性故障（权限缺失）才会真正熔断。
pub struct ReactionGate {
    consecutive_failures: AtomicU32,
}

impl ReactionGate {
    pub fn new() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
        }
    }

    fn is_open(&self) -> bool {
        self.consecutive_failures.load(Ordering::Relaxed) < MAX_FAILURES
    }

    fn record(&self, ok: bool) {
        if ok {
            self.consecutive_failures.store(0, Ordering::Relaxed);
        } else {
            let n = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
            if n == MAX_FAILURES {
                log::warn!(
                    target: "feishu",
                    "表情标记连续 {MAX_FAILURES} 次失败，已停用。\
                     多半是缺少 im:message.reaction:write 权限"
                );
            }
        }
    }
}

impl Default for ReactionGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 单条消息的表情状态。生命周期跟随一次消息处理
pub struct ReactionTracker<'a> {
    api: &'a FeishuApi,
    gate: &'a ReactionGate,
    message_id: &'a str,
    /// 当前挂着的那个表情的 reaction_id，切换时用它撤旧
    current: Option<String>,
}

impl<'a> ReactionTracker<'a> {
    pub fn new(api: &'a FeishuApi, gate: &'a ReactionGate, message_id: &'a str) -> Self {
        Self {
            api,
            gate,
            message_id,
            current: None,
        }
    }

    /// 切换到新表情：先撤掉当前的，再打上新的。
    ///
    /// `emoji` 为空表示该状态被用户关掉了，此时只撤不打。
    /// 返回是否真的打上了 —— 排队超限那种场景据此决定要不要回退发文本。
    pub async fn set(&mut self, emoji: &str) -> bool {
        if self.message_id.is_empty() || !self.gate.is_open() {
            return false;
        }

        // 撤销失败不影响打新的：多半是 id 已过期或表情已被人手动撤掉，
        // 继续往下走顶多是消息上多挂一个旧表情，比直接放弃要好
        if let Some(id) = self.current.take() {
            if let Err(e) = self.api.remove_reaction(self.message_id, &id).await {
                log::debug!(target: "feishu", "撤销表情失败: {e:#}");
            }
        }

        let emoji = emoji.trim();
        if emoji.is_empty() {
            return false;
        }

        match self.api.add_reaction(self.message_id, emoji).await {
            Ok(id) => {
                self.gate.record(true);
                // 没拿到 reaction_id 就没法再撤销它，留 None 即可，
                // 下次切换时新表情会直接叠加上去
                self.current = if id.is_empty() { None } else { Some(id) };
                true
            }
            Err(e) => {
                self.gate.record(false);
                log::warn!(target: "feishu", "打表情 {emoji} 失败: {e:#}");
                false
            }
        }
    }
}
