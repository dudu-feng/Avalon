// QQ 输出适配
//
// 按会话类型固定分流：
//   - 群聊：没有流式接口，攒完整段后按 2000 字符分片，最多 5 条被动回复；
//   - C2C：默认走 /stream_messages 原生流式，首帧失败自动退回普通分片。
//
// EngineEvent 由同步闭包产生，里面不能 await，所以网络请求都在本模块的 async
// 方法里完成；处理器每次收到 MessageDelta 调用一次 push。
//
// 被动回复的关键约束：同一个 msg_id 5 分钟内最多 5 条，msg_seq 必须递增。
// 流式本身占一条（seq=1），后续补发从 seq=2 开始。

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::api::QqApi;
use super::proto::Target;

/// 单条普通消息的字符上限。官方未公布统一值，社区普遍按 2000 保守分片
pub const TEXT_LIMIT: usize = 2000;
/// 同一个 msg_id 最多 5 条被动回复
pub const MAX_REPLIES: u32 = 5;
/// C2C 单次流式约 4096 字符上限，留 300 字符余量
pub const STREAM_LIMIT: usize = 3800;
/// 续帧节流。官方限 50 QPS，500ms 远低于上限，给网络抖动留足空间
const STREAM_THROTTLE: Duration = Duration::from_millis(500);
/// 首帧前至少攒这么多字符。逐字开流会先甩出「能…」这种碎片，
/// 短回复又根本来不及展示流式效果，不如直接走普通消息
const FIRST_STREAM_MIN_CHARS: usize = 24;
/// 兜底：首个 delta 后最多等这么久就开流，长回复不会因为字符数不够迟迟不动
const FIRST_STREAM_DELAY: Duration = Duration::from_millis(800);
/// 首帧之后至少间隔这么久再发结束帧，避免服务端流状态还没建好就收尾
const FINAL_FRAME_GAP: Duration = Duration::from_millis(300);
/// 被动回复窗口约 5 分钟。提前 30 秒停止推流，避免临界失败
const STREAM_DEADLINE: Duration = Duration::from_secs(270);
/// 流式消息占用被动回复的第 1 条；续帧与结束帧必须复用同一个序号
const STREAM_SEQ: u32 = 1;
/// 首选的流式内容类型。Markdown 直接交给 QQ 渲染，避免用户看到裸的 `**`
const STREAM_CONTENT_TYPE: &str = "markdown";
/// 内容被截断时的提示
const TRUNCATED_SUFFIX: &str = "\n\n…（内容过长，已截断）";

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamState {
    /// 还没发过任何帧
    Idle,
    /// 已拿到 stream_msg_id，后续 index 递增
    Streaming { id: String, index: u32 },
    /// 已正常收尾
    Finished,
    /// 首帧失败，退回非流式
    Failed,
    /// 已推过帧但后续失败，不能重发全文，只能提示中断
    Broken,
}

pub struct QqOut {
    api: QqApi,
    target: Target,
    msg_id: String,
    stream_enabled: bool,
    text: String,
    /// Done.result.message 的兜底正文
    fallback: String,
    /// 下一条普通消息的 msg_seq
    next_seq: u32,
    /// 已占用的被动回复条数，含流式那条
    replies: u32,
    stream: StreamState,
    /// 当前流的内容类型。Markdown 被拒时会降为 text，后续帧必须保持一致
    stream_content_type: &'static str,
    last_push: Instant,
    /// 第一个正文增量到达的时间。首帧延迟按它算，不能用 QqOut 创建时间，
    /// 否则模型思考几秒后第一个 delta 会立刻触发开流
    first_delta_at: Option<Instant>,
    started: Instant,
    /// 流式已成功下发的字符数，用于断流后只补发剩余部分
    sent_chars: usize,
    /// 是否实际向用户发出过内容
    sent_any: bool,
}

impl QqOut {
    pub fn new(api: QqApi, target: Target, msg_id: String, stream_enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            api,
            target,
            msg_id,
            stream_enabled,
            text: String::new(),
            fallback: String::new(),
            next_seq: 1,
            replies: 0,
            stream: StreamState::Idle,
            stream_content_type: STREAM_CONTENT_TYPE,
            last_push: now,
            first_delta_at: None,
            started: now,
            sent_chars: 0,
            sent_any: false,
        }
    }

    /// 正文增量。C2C 流式会在这里节流推帧；群聊只累积。
    pub async fn push(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.text.push_str(delta);

        if !self.stream_enabled {
            return;
        }
        if self.first_delta_at.is_none() {
            self.first_delta_at = Some(Instant::now());
        }
        self.pump_stream(false).await;
    }

    /// Done 事件的兜底正文：模型不走增量、只在结束时一次性给全文时使用
    pub fn set_fallback(&mut self, text: &str) {
        self.fallback = text.to_string();
    }

    /// 收尾。`error` 是 Engine 返回的致命错误，会追加到正文。
    /// 返回是否向用户发出过内容 —— 处理器据此决定要不要补一句兜底。
    pub async fn finish(mut self, error: Option<&str>) -> bool {
        if let Some(e) = error {
            if self.text.trim().is_empty() {
                self.text = format!("处理出错了：{e}");
            } else {
                self.text.push_str(&format!("\n\n处理出错：{e}"));
            }
        }

        if self.text.trim().is_empty() {
            self.text = self.fallback.clone();
        }
        if self.text.trim().is_empty() {
            self.text = "（这次没有产出内容）".to_string();
        }

        match self.stream.clone() {
            StreamState::Streaming { .. } => {
                // 首帧后立刻收尾可能撞上服务端还没建好流状态，留一个最小间隔
                let gap = FINAL_FRAME_GAP.saturating_sub(self.last_push.elapsed());
                if !gap.is_zero() {
                    tokio::time::sleep(gap).await;
                }
                // 已推过帧：只尝试一次收尾，失败也不重发全文
                if self.final_frame().await {
                    self.stream = StreamState::Finished;
                    self.sent_any = true;
                } else {
                    self.stream = StreamState::Broken;
                }
            }
            StreamState::Finished => {}
            StreamState::Broken => {}
            StreamState::Idle | StreamState::Failed => {
                // 首帧没成功：按普通分片发送全文
                self.send_all().await;
            }
        }

        // 流式走到一半断了/提前收尾：只把没下发过的尾部补出去，绝不重发全文
        if matches!(
            self.stream,
            StreamState::Finished | StreamState::Broken | StreamState::Streaming { .. }
        ) {
            self.send_remainder().await;
        }

        self.sent_any
    }

    /// 首帧是否值得开：攒够字符，或者第一个增量后已经等够时间
    fn ready_to_open_stream(&self) -> bool {
        self.text.chars().count() >= FIRST_STREAM_MIN_CHARS
            || self
                .first_delta_at
                .map(|t| t.elapsed() >= FIRST_STREAM_DELAY)
                .unwrap_or(false)
    }

    /// 尝试推流。`force` = 收尾/超长时跳过节流。
    async fn pump_stream(&mut self, force: bool) {
        if self.text.trim().is_empty() {
            return;
        }

        match self.stream.clone() {
            StreamState::Idle => {
                // 内容还太少、时间也太短：先攒着。短回复最终会走普通消息，
                // 既不会出现「能…」碎片，PC 端也能正常看到
                if !force && !self.ready_to_open_stream() {
                    return;
                }
                // 被动回复窗口快到了就别再开流，直接攒到结束走普通消息
                if self.started.elapsed() >= STREAM_DEADLINE {
                    self.stream = StreamState::Failed;
                    return;
                }

                let content = take_chars(&self.text, STREAM_LIMIT);
                let mut result = self
                    .api
                    .stream_c2c(
                        &self.target.chat_id,
                        &stream_body(
                            &self.msg_id,
                            STREAM_SEQ,
                            self.stream_content_type,
                            1,
                            0,
                            &content,
                            None,
                        ),
                    )
                    .await;

                // Markdown 流式被拒时退回纯文本流式，至少保住流式体验
                if let Err(e) = &result {
                    if self.stream_content_type != "text" {
                        log::info!(target: "qq", "Markdown 流式首帧失败，改用纯文本重试: {e:#}");
                        self.stream_content_type = "text";
                        result = self
                            .api
                            .stream_c2c(
                                &self.target.chat_id,
                                &stream_body(
                                    &self.msg_id,
                                    STREAM_SEQ,
                                    self.stream_content_type,
                                    1,
                                    0,
                                    &content,
                                    None,
                                ),
                            )
                            .await;
                    }
                }

                match result {
                    Ok(resp) => {
                        let id = resp
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if id.is_empty() {
                            log::warn!(
                                target: "qq",
                                "C2C 流式首帧未返回 stream_msg_id，退回非流式"
                            );
                            self.stream = StreamState::Failed;
                            return;
                        }
                        // 流式占用了被动回复的第 1 条，后续消息必须从 seq=2 开始
                        self.replies = 1;
                        self.next_seq = 2;
                        self.sent_any = true;
                        self.sent_chars = content.chars().count();
                        self.last_push = Instant::now();
                        self.stream = StreamState::Streaming { id, index: 1 };

                        // 首帧内容本身已经超过流式上限：立刻收尾，余下部分最后补发
                        if self.text.chars().count() > STREAM_LIMIT {
                            self.finish_stream_at_limit().await;
                        }
                    }
                    Err(e) => {
                        // 能力未开通、权限不足、网络失败都在这里退回非流式
                        log::warn!(target: "qq", "C2C 流式首帧失败，退回非流式: {e:#}");
                        self.stream = StreamState::Failed;
                    }
                }
            }
            StreamState::Streaming { id, index } => {
                if !force && self.last_push.elapsed() < STREAM_THROTTLE {
                    return;
                }
                if self.started.elapsed() >= STREAM_DEADLINE {
                    self.finish_stream_at_limit().await;
                    return;
                }

                if self.text.chars().count() > STREAM_LIMIT {
                    self.finish_stream_at_limit().await;
                    return;
                }

                let content = self.text.clone();
                match self
                    .api
                    .stream_c2c(
                        &self.target.chat_id,
                        &stream_body(
                            &self.msg_id,
                            STREAM_SEQ,
                            self.stream_content_type,
                            1,
                            index,
                            &content,
                            Some(&id),
                        ),
                    )
                    .await
                {
                    Ok(_) => {
                        self.sent_chars = content.chars().count();
                        self.last_push = Instant::now();
                        self.stream = StreamState::Streaming {
                            id,
                            index: index + 1,
                        };
                    }
                    Err(e) => {
                        log::warn!(target: "qq", "C2C 流式续帧失败: {e:#}");
                        self.stream = StreamState::Broken;
                        // 尽力关一次流，避免用户侧一直停在生成中
                        let _ = self
                            .api
                            .stream_c2c(
                                &self.target.chat_id,
                                &stream_body(
                                    &self.msg_id,
                                    STREAM_SEQ,
                                    self.stream_content_type,
                                    10,
                                    index,
                                    &content,
                                    Some(&id),
                                ),
                            )
                            .await;
                    }
                }
            }
            StreamState::Finished | StreamState::Failed | StreamState::Broken => {}
        }
    }

    /// 超过流式上限或临近超时：用截断内容收尾，余下部分记下来最后补发
    async fn finish_stream_at_limit(&mut self) {
        let StreamState::Streaming { id, index } = self.stream.clone() else {
            return;
        };

        let content = take_chars(&self.text, STREAM_LIMIT);
        if self
            .api
            .stream_c2c(
                &self.target.chat_id,
                &stream_body(
                    &self.msg_id,
                    STREAM_SEQ,
                    self.stream_content_type,
                    10,
                    index,
                    &content,
                    Some(&id),
                ),
            )
            .await
            .is_ok()
        {
            self.sent_chars = content.chars().count();
            self.stream = StreamState::Finished;
        } else {
            log::warn!(target: "qq", "C2C 流式超长收尾失败");
            self.stream = StreamState::Broken;
        }
    }

    /// 发送 input_state=10 的最终帧
    async fn final_frame(&mut self) -> bool {
        let StreamState::Streaming { id, index } = self.stream.clone() else {
            return false;
        };

        let content = take_chars(&self.text, STREAM_LIMIT);
        let body = stream_body(
            &self.msg_id,
            STREAM_SEQ,
            self.stream_content_type,
            10,
            index,
            &content,
            Some(&id),
        );
        match self.api.stream_c2c(&self.target.chat_id, &body).await {
            Ok(_) => {
                self.sent_chars = content.chars().count();
                true
            }
            Err(e) => {
                log::warn!(target: "qq", "C2C 流式收尾失败: {e:#}");
                false
            }
        }
    }

    /// 补发流式截断/断流后剩下的内容
    async fn send_remainder(&mut self) {
        let rest: String = self.text.chars().skip(self.sent_chars).collect();
        if rest.trim().is_empty() {
            return;
        }
        if !self.send_chunks(&rest).await {
            self.send_notice("（输出中断）").await;
        }
    }

    /// 普通分片发送全文
    async fn send_all(&mut self) {
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.send_chunks(&text).await;
    }

    /// 返回是否至少成功发出一条
    async fn send_chunks(&mut self, text: &str) -> bool {
        let remaining = MAX_REPLIES.saturating_sub(self.replies);
        if remaining == 0 {
            log::warn!(target: "qq", "被动回复条数已用完，剩余内容未发送");
            return false;
        }

        let mut sent = false;
        let (chunks, _truncated) = split_text(text, TEXT_LIMIT, remaining as usize);
        for chunk in chunks {
            if self.replies >= MAX_REPLIES {
                break;
            }
            let seq = self.next_seq;
            self.next_seq += 1;
            self.replies += 1;
            match self
                .api
                .send_text(&self.target, &chunk, &self.msg_id, seq, None)
                .await
            {
                Ok(_) => {
                    sent = true;
                    self.sent_any = true;
                }
                Err(e) => {
                    // 被动回复超限/超频不做重试轰炸，记日志即可
                    log::warn!(target: "qq", "发送 QQ 消息失败 seq={seq}: {e:#}");
                    break;
                }
            }
        }
        sent
    }

    /// 发一条短提示，仍受被动回复条数限制
    async fn send_notice(&mut self, text: &str) {
        if self.replies >= MAX_REPLIES {
            return;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.replies += 1;
        if self
            .api
            .send_text(&self.target, text, &self.msg_id, seq, None)
            .await
            .is_ok()
        {
            self.sent_any = true;
        }
    }
}

/// 构造 stream_messages 请求体。
///
/// 每一帧都要带同一个 `msg_id` + `msg_seq`：这是被动回复的锚点，也是服务端把
/// 多帧归到同一条流上的依据。只带 `stream_msg_id` 不带锚点时，首帧能成功，
/// 但续帧会被服务端判成无主请求并返回 500（50015001 系统繁忙）。
/// 首帧不传 `stream_msg_id`；续帧/结束帧必须传。内容用 replace 语义，传当前全文。
fn stream_body(
    msg_id: &str,
    msg_seq: u32,
    content_type: &str,
    input_state: u32,
    index: u32,
    content: &str,
    stream_msg_id: Option<&str>,
) -> Value {
    let mut body = json!({
        "input_state": input_state,
        "input_mode": "replace",
        "index": index,
        "content_type": content_type,
        "content_raw": content,
    });

    if !msg_id.is_empty() {
        body["msg_id"] = Value::String(msg_id.to_string());
        body["msg_seq"] = Value::Number(msg_seq.into());
    }
    if let Some(id) = stream_msg_id {
        body["stream_msg_id"] = Value::String(id.to_string());
    }
    body
}

/// 按字符（而非字节）截断
fn take_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// 文本分片。
///
/// 优先在段落、换行、空格处切；边界太靠前（不到半片）就宁可硬切，避免产出
/// 一堆很短的碎片。返回 (分片, 是否因超过 max_chunks 而截断)。
pub fn split_text(text: &str, limit: usize, max_chunks: usize) -> (Vec<String>, bool) {
    let text = text.trim();
    if text.is_empty() || max_chunks == 0 || limit == 0 {
        return (Vec::new(), false);
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0usize;

    while start < chars.len() && out.len() < max_chunks {
        let remaining = chars.len() - start;
        if remaining <= limit {
            let chunk: String = chars[start..].iter().collect();
            push_trimmed(&mut out, chunk);
            start = chars.len();
            break;
        }

        let window: String = chars[start..start + limit].iter().collect();
        let boundary = find_boundary(&window).filter(|cut| *cut >= window.len() / 2);
        let cut = boundary.unwrap_or(window.len());
        let consumed_chars = window[..cut].chars().count().max(1);
        let chunk: String = chars[start..start + consumed_chars].iter().collect();
        push_trimmed(&mut out, chunk);
        start += consumed_chars;
    }

    let truncated = start < chars.len();
    if truncated {
        if let Some(last) = out.last_mut() {
            let keep = limit.saturating_sub(TRUNCATED_SUFFIX.chars().count());
            let mut s: String = last.chars().take(keep).collect();
            s.push_str(TRUNCATED_SUFFIX);
            *last = s;
        }
    }
    (out, truncated)
}

/// 在窗口里找最后一个自然边界，返回字节下标
fn find_boundary(window: &str) -> Option<usize> {
    window
        .rfind("\n\n")
        .map(|i| i + 2)
        .or_else(|| window.rfind('\n').map(|i| i + 1))
        .or_else(|| window.rfind(' ').map(|i| i + 1))
}

fn push_trimmed(out: &mut Vec<String>, chunk: String) {
    let trimmed = chunk.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        let (chunks, truncated) = split_text("你好", TEXT_LIMIT, 5);
        assert_eq!(chunks, vec!["你好"]);
        assert!(!truncated);
    }

    #[test]
    fn splits_at_paragraph_boundary() {
        let text = format!("{}\n\n{}", "a".repeat(1500), "b".repeat(1500));
        let (chunks, truncated) = split_text(&text, TEXT_LIMIT, 5);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(1500));
        assert_eq!(chunks[1], "b".repeat(1500));
        assert!(!truncated);
    }

    #[test]
    fn unicode_is_not_split_mid_character() {
        let text = "中".repeat(4500);
        let (chunks, truncated) = split_text(&text, TEXT_LIMIT, 5);
        assert!(chunks.iter().all(|c| c.chars().all(|ch| ch == '中')));
        assert_eq!(
            chunks.iter().map(|c| c.chars().count()).sum::<usize>(),
            4500
        );
        assert!(!truncated);
    }

    #[test]
    fn caps_at_max_chunks_and_marks_truncated() {
        let text = "x".repeat(TEXT_LIMIT * 8);
        let (chunks, truncated) = split_text(&text, TEXT_LIMIT, 5);
        assert_eq!(chunks.len(), 5);
        assert!(truncated);
        assert!(chunks.last().unwrap().ends_with("已截断）"));
    }

    #[test]
    fn first_stream_frame_has_msg_id_but_no_stream_id() {
        let body = stream_body("M1", 1, "markdown", 1, 0, "hi", None);
        assert_eq!(body["msg_id"], "M1");
        assert_eq!(body["msg_seq"], 1);
        assert_eq!(body["content_type"], "markdown");
        assert_eq!(body["input_state"], 1);
        assert_eq!(body["index"], 0);
        assert!(body.get("stream_msg_id").is_none());
        assert_eq!(body["input_mode"], "replace");
        assert_eq!(body["content_raw"], "hi");
    }

    #[test]
    fn continuation_and_end_frames_carry_stream_id() {
        let body = stream_body("M1", 1, "markdown", 10, 3, "hi", Some("S1"));
        assert_eq!(body["stream_msg_id"], "S1");
        assert_eq!(body["content_type"], "markdown");
        assert_eq!(body["input_state"], 10);
        assert_eq!(body["index"], 3);
        // 续帧必须继续带同一个锚点，否则服务端会返回 50015001
        assert_eq!(body["msg_id"], "M1");
        assert_eq!(body["msg_seq"], 1);
    }
}
