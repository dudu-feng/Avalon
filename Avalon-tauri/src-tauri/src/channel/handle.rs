// 渠道对外句柄
//
// 解决的是生命周期错配：FeishuApi / QqApi 只活在各自 run() 的作用域里（渠道可随时启停），
// 而工具侧（ToolSet）是应用启动时就组装好、一直存在的。工具不能持有一份可能已失效的
// clone，所以中间放一个槽位 —— 渠道起来时填进去，停止时清空，工具每次调用现取。
//
// 「停止渠道」= 清空槽位 = 工具报「渠道未运行」。这是有意的：停止的语义就是
// 「在这个渠道上安静下来」，此时还能靠工具发消息会让这个开关名不副实。
//
// 除了发送能力，句柄还记录最近一次成功/失败与限流余量。工具排查失败原因时
// 不用靠猜：渠道在不在、额度还剩多少、上一次到底报了什么都能原样拿回来。

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use super::feishu::api::{error_hint, FeishuApi, Target};
use super::qq::official::proto::{ChatType, Target as QqTarget};
use super::qq::official::QqApi;

/// 飞书限流窗口长度
const FEISHU_WINDOW: Duration = Duration::from_secs(60);
/// 飞书单窗口允许发送条数
const FEISHU_WINDOW_LIMIT: u32 = 10;
/// QQ 主动消息更贵：平台侧额度有限，本地再收一道口子
const QQ_WINDOW: Duration = Duration::from_secs(60);
const QQ_WINDOW_LIMIT: u32 = 5;

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 带时间点的错误记录。时间放在前面，模型读到的第一句就是「什么时候失败的」
fn stamp(message: impl Into<String>) -> String {
    format!("{} {}", now(), message.into())
}

/// 渠道运行期共享给工具层的飞书句柄。
///
/// 只暴露「发消息」这一件事，不把 FeishuApi 整个漏出去 ——
/// 工具没有理由去动 token、卡片流式更新那些渠道内部机制。
pub struct FeishuHandle {
    api: RwLock<Option<FeishuApi>>,
    /// 限流窗口：(窗口起点, 本窗口已发条数)
    window: Mutex<(Instant, u32)>,
    last_error: Mutex<Option<String>>,
    last_success: Mutex<Option<String>>,
}

/// 发送方式。普通发送与「回复某条消息」的降级路径必须一致，
/// 用枚举把两条端点选择收在一处，避免 send / send_reply 各写一遍兜底
#[derive(Clone, Copy)]
enum SendKind<'a> {
    /// 发到会话或用户
    To(&'a Target),
    /// 回复指定消息 id
    Reply(&'a str),
}

impl FeishuHandle {
    pub fn new() -> Self {
        Self {
            api: RwLock::new(None),
            window: Mutex::new((Instant::now(), 0)),
            last_error: Mutex::new(None),
            last_success: Mutex::new(None),
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

    /// 渠道任务是否在跑。注意这是「任务存在」，不是「长连接此刻已就绪」，
    /// 重连中的渠道仍算运行中，消息可能稍后才发出。
    pub fn is_online(&self) -> bool {
        self.api.read().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// 最近一次发送失败（成功后清空，代表当前仍是坏的）
    pub fn last_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 最近一次发送成功的时间
    pub fn last_success(&self) -> Option<String> {
        self.last_success
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 当前限流窗口还剩多少条
    pub fn rate_remaining(&self) -> u32 {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = &mut *w;
        if start.elapsed() >= FEISHU_WINDOW {
            *start = Instant::now();
            *count = 0;
        }
        FEISHU_WINDOW_LIMIT.saturating_sub(*count)
    }

    /// 发一条消息，返回 message_id。
    ///
    /// 优先内联卡片：模型输出必然带 markdown，飞书 text 消息不渲染，
    /// `**粗体**`、代码块会原样露在用户手机上。卡片被权限或参数校验拦住时
    /// 退回纯文本 —— 对「提醒主人」这类场景，能收到比格式好看重要。
    pub async fn send(&self, target: &Target, text: &str) -> Result<String> {
        let api = self.api_snapshot()?;
        self.check_rate()?;
        self.send_with_fallback(&api, SendKind::To(target), text)
            .await
    }

    /// 回复某条消息。群聊引用触发消息走这个端点，会话 id 由消息 id 反推
    pub async fn send_reply(&self, message_id: &str, text: &str) -> Result<String> {
        let api = self.api_snapshot()?;
        self.check_rate()?;
        self.send_with_fallback(&api, SendKind::Reply(message_id), text)
            .await
    }

    /// 先把 api clone 出锁，guard 到这一行就结束。绝不能跨 await 持有读锁 ——
    /// 一次几十秒的 HTTP 会把 stop() 要拿的写锁堵死
    fn api_snapshot(&self) -> Result<FeishuApi> {
        let api = self.api.read().unwrap_or_else(|e| e.into_inner()).clone();
        match api {
            Some(api) => Ok(api),
            None => bail!("飞书渠道未运行，请先在托盘或设置页启动渠道"),
        }
    }

    /// 卡片优先、纯文本兜底。普通发送与回复共用这段逻辑，
    /// 只是两条路径调用的端点不同，降级时必须落在同一个目标上
    async fn send_with_fallback(
        &self,
        api: &FeishuApi,
        kind: SendKind<'_>,
        text: &str,
    ) -> Result<String> {
        let first = match kind {
            SendKind::To(target) => api.send_markdown_to(target, text).await,
            SendKind::Reply(id) => api.reply_markdown_to(id, text).await,
        };
        match first {
            Ok(id) => {
                self.note_success();
                Ok(id)
            }
            Err(card_err) => {
                // 网络层错误（超时、DNS、连接被拒）没有业务码，再走一次纯文本只会
                // 把 30 秒超时翻倍；只有飞书明确返回了业务错误才值得换消息类型重试
                let raw = format!("{card_err:#}");
                if !raw.contains("code=") {
                    self.note_error(raw);
                    return Err(card_err);
                }
                log::warn!(target: "feishu", "卡片消息发送失败，改发纯文本: {raw}");
                let second = match kind {
                    SendKind::To(target) => api.send_text_to(target, text).await,
                    SendKind::Reply(id) => api.reply_text_to(id, text).await,
                };
                match second {
                    Ok(id) => {
                        self.note_success();
                        Ok(id)
                    }
                    Err(text_err) => {
                        let message = format_send_error(&card_err, &text_err);
                        self.note_error(message.clone());
                        bail!("{message}");
                    }
                }
            }
        }
    }

    fn note_success(&self) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.last_success.lock().unwrap_or_else(|e| e.into_inner()) = Some(now());
    }

    fn note_error(&self, message: String) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(stamp(message));
    }

    /// 固定窗口限流。
    ///
    /// 不是杞人忧天：ReAct 主循环没有最大轮数上限，模型陷进「调用失败→重试」时
    /// 能无限次调工具，而定时任务是无人值守跑的 —— 没人在旁边按停止。
    /// 超限返回错误而不是静默丢弃，让模型在 tool result 里看见并停手。
    fn check_rate(&self) -> Result<()> {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = &mut *w;
        if start.elapsed() >= FEISHU_WINDOW {
            *start = Instant::now();
            *count = 0;
        }
        if *count >= FEISHU_WINDOW_LIMIT {
            let message = format!(
                "发送过于频繁，{FEISHU_WINDOW_LIMIT} 条/分钟的上限已用完，请稍后再试或改为一条消息汇总"
            );
            drop(w);
            self.note_error(message.clone());
            bail!("{message}");
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

/// QQ 对外句柄。
///
/// 和飞书不同，QQ 渠道没有「回复正文自动发送」之外的共享出口：
/// 主动消息就是一次普通 C2C 发送（不带 msg_id），平台侧另有额度限制。
/// 因此这里的限流比飞书更紧（5 条/分钟），避免模型把主动额度打光。
pub struct QqHandle {
    api: RwLock<Option<QqApi>>,
    window: Mutex<(Instant, u32)>,
    last_error: Mutex<Option<String>>,
    last_success: Mutex<Option<String>>,
}

impl QqHandle {
    pub fn new() -> Self {
        Self {
            api: RwLock::new(None),
            window: Mutex::new((Instant::now(), 0)),
            last_error: Mutex::new(None),
            last_success: Mutex::new(None),
        }
    }

    pub fn set(&self, api: QqApi) {
        *self.api.write().unwrap_or_else(|e| e.into_inner()) = Some(api);
    }

    pub fn clear(&self) {
        *self.api.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn is_online(&self) -> bool {
        self.api.read().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn last_success(&self) -> Option<String> {
        self.last_success
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn rate_remaining(&self) -> u32 {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = &mut *w;
        if start.elapsed() >= QQ_WINDOW {
            *start = Instant::now();
            *count = 0;
        }
        QQ_WINDOW_LIMIT.saturating_sub(*count)
    }

    /// 给指定 QQ 会话（群聊或单聊）发一条主动消息（不携带 msg_id）。
    ///
    /// 主动消息受 QQ 平台额度限制，通常需要用户/群近期和机器人有过互动；
    /// 额度耗尽或用户未互动时平台会拒绝，错误原样带回给模型。
    /// 群聊与单聊使用同一个 `Target`，由调用方明确 chat_type，避免把用户
    /// openid 当成群 openid 发出去 —— QQ 的两种 openid 都是不透明字符串，
    /// 服务端无法替我们判别。
    pub async fn send_to(
        &self,
        target: &QqTarget,
        text: &str,
        quote_message_id: Option<&str>,
    ) -> Result<String> {
        let api = self.api.read().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(api) = api else {
            bail!("QQ 渠道未运行，请先在托盘或设置页启动渠道");
        };

        self.check_rate()?;
        // msg_id 为空 = 主动消息；quote_message_id 是引用气泡用的 REFIDX_ id
        match api.send_text(target, text, "", 0, quote_message_id).await {
            Ok(id) => {
                self.note_success();
                Ok(id)
            }
            Err(e) => {
                let message = format!("{e:#}");
                self.note_error(message.clone());
                Err(e)
            }
        }
    }

    /// 给主人发一条主动 C2C 消息。收件人来自配置，工具层不暴露任意用户 id
    pub async fn send_to_owner(&self, open_id: &str, text: &str) -> Result<String> {
        let open_id = open_id.trim();
        if open_id.is_empty() {
            bail!("QQ owner_open_id 为空");
        }
        self.send_to(&QqTarget::new(ChatType::C2c, open_id), text, None)
            .await
    }

    fn note_success(&self) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.last_success.lock().unwrap_or_else(|e| e.into_inner()) = Some(now());
    }

    fn note_error(&self, message: String) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(stamp(message));
    }

    fn check_rate(&self) -> Result<()> {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        let (start, count) = &mut *w;
        if start.elapsed() >= QQ_WINDOW {
            *start = Instant::now();
            *count = 0;
        }
        if *count >= QQ_WINDOW_LIMIT {
            let message = format!(
                "QQ 主动消息发送过于频繁，{QQ_WINDOW_LIMIT} 条/分钟的上限已用完。\
                 平台主动消息额度有限，请合并为一条消息稍后再试"
            );
            drop(w);
            self.note_error(message.clone());
            bail!("{message}");
        }
        *count += 1;
        Ok(())
    }
}

impl Default for QqHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// 飞书失败文案：卡片与纯文本两条路都失败时，把两段原因都留下，
/// 并翻译常见业务码，避免模型只看到一串 code 数字又去盲目重试。
fn format_send_error(card_err: &anyhow::Error, text_err: &anyhow::Error) -> String {
    let card = format!("{card_err:#}");
    let text = format!("{text_err:#}");
    let mut message = format!("卡片发送失败：{card}；纯文本兜底也失败：{text}");

    for raw in [&card, &text] {
        if let Some(hint) = error_hint(raw) {
            // 两条路的错误码通常相同，命中一次就够
            message.push_str(&format!("\n可能的解决办法：{hint}"));
            break;
        }
    }
    message
}

/// 渠道任务结束时清空句柄的 RAII 兜底。
///
/// 为什么不只在 ChannelManager::stop() 里显式清：长连接遇到致命错误会自己 return，
/// 这条路上 stop() 根本不会被调用，槽位就会留着一个「看起来在线其实没连接」的 api。
/// 而 Drop 在 task.abort() 下也可靠 —— future 被丢弃时，状态机里存活的局部变量
/// 一定析构，即使它正卡在某个 await 上。
pub struct HandleGuard(pub Arc<FeishuHandle>);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

pub struct QqHandleGuard(pub Arc<QqHandle>);

impl Drop for QqHandleGuard {
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
        for _ in 0..FEISHU_WINDOW_LIMIT {
            assert!(h.check_rate().is_ok());
        }
    }

    #[test]
    fn 限流超限后拒绝并给出可读文案() {
        let h = FeishuHandle::new();
        for _ in 0..FEISHU_WINDOW_LIMIT {
            let _ = h.check_rate();
        }
        let err = h.check_rate().unwrap_err().to_string();
        assert!(err.contains("过于频繁"));
    }

    #[test]
    fn 窗口过期后重新放行() {
        let h = FeishuHandle::new();
        for _ in 0..FEISHU_WINDOW_LIMIT {
            let _ = h.check_rate();
        }
        assert!(h.check_rate().is_err());
        // 把窗口起点往前拨，模拟时间流逝，不用真的 sleep 60 秒
        {
            let mut w = h.window.lock().unwrap();
            w.0 = Instant::now() - FEISHU_WINDOW - Duration::from_secs(1);
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

    #[test]
    fn 诊断信息初始为空且限额可见() {
        let h = FeishuHandle::new();
        assert!(!h.is_online());
        assert!(h.last_error().is_none());
        assert!(h.last_success().is_none());
        assert_eq!(h.rate_remaining(), FEISHU_WINDOW_LIMIT);
    }

    #[test]
    fn qq句柄离线时给出渠道未运行() {
        let h = QqHandle::new();
        let err = tauri::async_runtime::block_on(h.send_to_owner("U1", "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("未运行"));
        assert!(!h.is_online());
    }
}
