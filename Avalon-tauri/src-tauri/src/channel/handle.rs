// 飞书对外句柄
//
// 解决的是生命周期错配：FeishuApi 只活在 feishu::run() 的作用域里（渠道可随时启停），
// 而工具侧（ToolSet）是应用启动时就组装好、一直存在的。工具不能持有一份可能已失效的
// clone，所以中间放一个槽位 —— 渠道起来时填进去，停止时清空，工具每次调用现取。
//
// 「停止渠道」= 清空槽位 = 工具报「渠道未运行」。这是有意的：停止的语义就是
// 「在飞书上安静下来」，此时还能靠工具发消息会让这个开关名不副实。

use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use super::feishu::api::{FeishuApi, Target};

/// 限流窗口长度
const WINDOW: Duration = Duration::from_secs(60);
/// 单个窗口内允许的发送条数
const WINDOW_LIMIT: u32 = 10;

/// 渠道运行期共享给工具层的句柄。
///
/// 只暴露「发消息」这一件事，不把 FeishuApi 整个漏出去 ——
/// 工具没有理由去动 token、卡片流式更新那些渠道内部机制。
pub struct FeishuHandle {
    api: RwLock<Option<FeishuApi>>,
    /// 限流窗口：(窗口起点, 本窗口已发条数)
    window: Mutex<(Instant, u32)>,
}

impl FeishuHandle {
    pub fn new() -> Self {
        Self {
            api: RwLock::new(None),
            window: Mutex::new((Instant::now(), 0)),
        }
    }

    /// 渠道启动后填入 api
    pub fn set(&self, api: FeishuApi) {
        *self.api.write().unwrap_or_else(|e| e.into_inner()) = Some(api);
    }

    /// 渠道停止时清空。必须保持同步 —— drop guard 里不能 await
    pub fn clear(&self) {
        *self.api.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// 发一条 markdown 消息，返回 message_id。
    ///
    /// 一律走卡片而非纯文本：模型输出必然带 markdown，飞书 text 消息不渲染，
    /// `**粗体**`、代码块会原样露在用户手机上。
    pub async fn send(&self, target: &Target, text: &str) -> Result<String> {
        // 先把 api clone 出锁，guard 到这一行就结束。绝不能跨 await 持有读锁 ——
        // 一次几十秒的 HTTP 会把 stop() 要拿的写锁堵死
        let api = self
            .api
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(api) = api else {
            bail!("飞书渠道未运行，请先在托盘或设置页启动渠道");
        };

        self.check_rate()?;
        api.send_markdown_to(target, text).await
    }

    /// 固定窗口限流。
    ///
    /// 不是杞人忧天：ReAct 主循环没有最大轮数上限，模型陷进「调用失败→重试」时
    /// 能无限次调工具，而定时任务是无人值守跑的 —— 没人在旁边按停止。
    /// 超限返回错误而不是静默丢弃，让模型在 tool result 里看见并停手。
    fn check_rate(&self) -> Result<()> {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = &mut *w;
        if start.elapsed() >= WINDOW {
            *start = Instant::now();
            *count = 0;
        }
        if *count >= WINDOW_LIMIT {
            bail!("发送过于频繁，{WINDOW_LIMIT} 条/分钟的上限已用完，请稍后再试或改为一条消息汇总");
        }
        *count += 1;
        Ok(())
    }
}

impl Default for FeishuHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// 渠道任务结束时清空句柄的 RAII 兜底。
///
/// 为什么不只在 ChannelManager::stop() 里显式清：长连接遇到致命错误会自己 return，
/// 这条路上 stop() 根本不会被调用，槽位就会留着一个「看起来在线其实没连接」的 api。
/// 而 Drop 在 task.abort() 下也可靠 —— future 被丢弃时，状态机里存活的局部变量
/// 一定析构，即使它正卡在某个 await 上。
pub struct HandleGuard(pub std::sync::Arc<FeishuHandle>);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 限流放行窗口内的前若干条() {
        let h = FeishuHandle::new();
        for _ in 0..WINDOW_LIMIT {
            assert!(h.check_rate().is_ok());
        }
    }

    #[test]
    fn 限流超限后拒绝并给出可读文案() {
        let h = FeishuHandle::new();
        for _ in 0..WINDOW_LIMIT {
            let _ = h.check_rate();
        }
        let err = h.check_rate().unwrap_err().to_string();
        assert!(err.contains("过于频繁"));
    }

    #[test]
    fn 窗口过期后重新放行() {
        let h = FeishuHandle::new();
        for _ in 0..WINDOW_LIMIT {
            let _ = h.check_rate();
        }
        assert!(h.check_rate().is_err());
        // 把窗口起点往前拨，模拟时间流逝，不用真的 sleep 60 秒
        {
            let mut w = h.window.lock().unwrap();
            w.0 = Instant::now() - WINDOW - Duration::from_secs(1);
        }
        assert!(h.check_rate().is_ok());
    }

    #[test]
    fn 离线时发送直接失败而不是挂起() {
        let h = FeishuHandle::new();
        let err = tauri::async_runtime::block_on(h.send(&Target::Chat("oc_x".into()), "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("未运行"));
    }
}
