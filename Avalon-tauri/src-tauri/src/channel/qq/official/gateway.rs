// QQ 官方机器人 WebSocket 网关
//
// 协议流程（与官方文档和社区 SDK 一致）：
//   1. GET /gateway 拿到 wss 地址
//   2. 连上后服务端下发 op 10 Hello，带 heartbeat_interval（毫秒）
//   3. 客户端发 op 2 Identify（首次）或 op 6 Resume（断线重连，带 session_id + seq）
//   4. 按 interval 发 op 1 Heartbeat（d = 最新 seq），服务端回 op 11 ACK
//   5. op 0 Dispatch 投递给 handler；op 7 要求立刻重连；op 9 清空会话后重新 Identify
//
// Identify 的 token 格式是 `QQBot {access_token}`，不是 QQ 频道那套
// `Bot {appid}.{token}`，写错会直接 4004。
//
// 断线重连优先 Resume：服务端会补发断线期间的事件，减少漏消息。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::api::QqApi;
use super::proto::GatewayEvent;
use crate::channel::ChannelStatus;

/// 服务端没给 Hello 时的兜底心跳间隔
const DEFAULT_HEARTBEAT_MS: u64 = 41_250;
/// 心跳间隔的合理范围，防止服务端下发 0 导致死循环
const MIN_HEARTBEAT_MS: u64 = 1_000;
const MAX_HEARTBEAT_MS: u64 = 300_000;
/// 重连等待上限
const MAX_BACKOFF: Duration = Duration::from_secs(30);

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>;

/// 事件 channel
pub type EventSender = mpsc::UnboundedSender<GatewayEvent>;

/// 跨重连保留的会话状态
#[derive(Default)]
struct Session {
    session_id: Option<String>,
    seq: Option<i64>,
}

impl Session {
    fn clear(&mut self) {
        self.session_id = None;
        self.seq = None;
    }
}

/// 连接过程中的错误分类
enum ConnError {
    /// 凭证错误、Intent 非法、机器人被封禁 —— 重试无意义
    Fatal(String),
    /// 网络抖动、服务端临时故障
    Retryable(String),
}

impl ConnError {
    fn message(&self) -> &str {
        match self {
            ConnError::Fatal(m) | ConnError::Retryable(m) => m,
        }
    }
}

/// 一次连接如何结束
enum Outcome {
    /// 正常断开，下次优先 Resume
    Closed,
    /// 服务端要求重连（op 7）
    Reconnect,
    /// 会话无效（op 9 / 4006-4009），清空会话后重新 Identify
    InvalidSession,
}

struct ConnResult {
    outcome: Outcome,
    /// 是否成功收到 READY / RESUMED。用于把退避计数归零
    ready: bool,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    op: i64,
    #[serde(default)]
    d: Value,
    #[serde(default)]
    s: Option<i64>,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

const OP_DISPATCH: i64 = 0;
const OP_HEARTBEAT: i64 = 1;
const OP_IDENTIFY: i64 = 2;
const OP_RESUME: i64 = 6;
const OP_RECONNECT: i64 = 7;
const OP_INVALID_SESSION: i64 = 9;
const OP_HELLO: i64 = 10;
const OP_HEARTBEAT_ACK: i64 = 11;

pub struct GatewayClient {
    api: QqApi,
    intents: i64,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ChannelStatus>>,
}

impl GatewayClient {
    pub fn new(
        api: QqApi,
        intents: i64,
        stop: Arc<AtomicBool>,
        status: Arc<Mutex<ChannelStatus>>,
    ) -> Self {
        Self {
            api,
            intents,
            stop,
            status,
        }
    }

    fn set_status(&self, s: ChannelStatus) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// 主循环：连接 → 收发 → 断开 → 退避重连，直到被要求停止或遇到致命错误
    pub async fn run(&self, events: EventSender) {
        let mut session = Session::default();
        let mut attempt: u32 = 0;

        loop {
            if self.stopped() {
                break;
            }

            self.set_status(if attempt == 0 {
                ChannelStatus::Connecting
            } else {
                ChannelStatus::Reconnecting
            });

            match self.connect_once(&events, &mut session).await {
                Ok(result) => {
                    if self.stopped() {
                        break;
                    }
                    if result.ready {
                        attempt = 0;
                    }
                    if let Outcome::InvalidSession = result.outcome {
                        log::info!(target: "qq", "会话已失效，重新 Identify");
                        session.clear();
                    } else {
                        log::info!(target: "qq", "网关连接已断开，准备重连（优先 Resume）");
                    }
                }
                Err(ConnError::Fatal(message)) => {
                    log::error!(target: "qq", "致命错误，停止 QQ 渠道: {message}");
                    self.set_status(ChannelStatus::Error { message });
                    return;
                }
                Err(e @ ConnError::Retryable(_)) => {
                    log::warn!(target: "qq", "网关连接失败: {}", e.message());
                }
            }

            if self.stopped() {
                break;
            }

            let wait = backoff(attempt);
            tokio::time::sleep(wait).await;
            attempt = attempt.saturating_add(1);
        }

        self.set_status(ChannelStatus::Stopped);
        log::info!(target: "qq", "QQ 网关已停止");
    }

    /// 建立一次连接并持续收发，直到断开
    async fn connect_once(
        &self,
        events: &EventSender,
        session: &mut Session,
    ) -> Result<ConnResult, ConnError> {
        let url = self.api.get_gateway().await.map_err(classify_api_error)?;
        let token = self.api.token().await.map_err(classify_api_error)?;
        let auth = format!("QQBot {token}");

        let (stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(|e| ConnError::Retryable(format!("QQ 网关握手失败: {e}")))?;

        let (mut write, mut read) = stream.split();
        log::info!(target: "qq", "QQ 网关已连接");

        let mut heartbeat_ms = DEFAULT_HEARTBEAT_MS;
        let mut ticker = tokio::time::interval(Duration::from_millis(heartbeat_ms));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // 首个 tick 立即就绪，先消费掉，等 Hello 再启动
        let mut hello = false;
        let mut ready = false;
        let mut awaiting_ack_since: Option<Instant> = None;

        let mut stop_tick = tokio::time::interval(Duration::from_millis(500));
        stop_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = stop_tick.tick() => {
                    if self.stopped() {
                        let _ = write.close().await;
                        return Ok(ConnResult { outcome: Outcome::Closed, ready });
                    }
                }

                _ = ticker.tick(), if hello => {
                    // 连续两轮没有收到 ACK，按断线处理
                    if let Some(since) = awaiting_ack_since {
                        if since.elapsed() > Duration::from_millis(heartbeat_ms).saturating_mul(2) {
                            return Err(ConnError::Retryable("心跳连续未收到 ACK".to_string()));
                        }
                    } else {
                        awaiting_ack_since = Some(Instant::now());
                    }

                    let d = match session.seq {
                        Some(seq) => Value::from(seq),
                        None => Value::Null,
                    };
                    send_json(&mut write, &json!({ "op": OP_HEARTBEAT, "d": d })).await?;
                }

                incoming = read.next() => {
                    let Some(msg) = incoming else {
                        return Ok(ConnResult { outcome: Outcome::Closed, ready });
                    };
                    let msg = msg.map_err(|e| {
                        ConnError::Retryable(format!("读取 QQ 网关帧失败: {e}"))
                    })?;

                    let text = match msg {
                        Message::Text(t) => t.to_string(),
                        // 部分版本会用 Binary 承载未压缩的 JSON，一并兼容
                        Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                            Ok(s) => s,
                            Err(_) => {
                                log::debug!(target: "qq", "收到非文本网关帧（可能是压缩帧），已忽略");
                                continue;
                            }
                        },
                        Message::Close(frame) => {
                            let (code, reason) = frame
                                .map(|f| (u16::from(f.code), f.reason.to_string()))
                                .unwrap_or((0, String::new()));
                            log::info!(target: "qq", "网关关闭 code={code} reason={reason}");
                            if let Some(fatal) = close_is_fatal(code) {
                                return Err(ConnError::Fatal(fatal.to_string()));
                            }
                            return Ok(ConnResult {
                                outcome: close_outcome(code),
                                ready,
                            });
                        }
                        // tungstenite 自动处理底层 ping/pong
                        _ => continue,
                    };

                    let payload: Payload = match serde_json::from_str(&text) {
                        Ok(p) => p,
                        Err(e) => {
                            log::warn!(target: "qq", "QQ 网关帧解析失败，已跳过: {e}");
                            continue;
                        }
                    };

                    if let Some(s) = payload.s {
                        session.seq = Some(s);
                    }

                    match payload.op {
                        OP_HELLO => {
                            heartbeat_ms = heartbeat_from(&payload.d);
                            ticker = tokio::time::interval(Duration::from_millis(heartbeat_ms));
                            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                            hello = true;
                            awaiting_ack_since = None;

                            let body = if session.session_id.is_some()
                                && session.seq.is_some_and(|s| s > 0)
                            {
                                json!({
                                    "op": OP_RESUME,
                                    "d": {
                                        "token": auth,
                                        "session_id": session.session_id,
                                        "seq": session.seq,
                                    }
                                })
                            } else {
                                json!({
                                    "op": OP_IDENTIFY,
                                    "d": {
                                        "token": auth,
                                        "intents": self.intents,
                                        "shard": [0, 1],
                                        "properties": {},
                                    }
                                })
                            };
                            log::info!(
                                target: "qq",
                                "Hello 收到，心跳 {} ms，发送 {}",
                                heartbeat_ms,
                                if session.session_id.is_some() { "Resume" } else { "Identify" }
                            );
                            send_json(&mut write, &body).await?;
                        }

                        OP_HEARTBEAT_ACK => {
                            awaiting_ack_since = None;
                        }

                        OP_RECONNECT => {
                            log::info!(target: "qq", "服务端要求重连");
                            return Ok(ConnResult { outcome: Outcome::Reconnect, ready });
                        }

                        OP_INVALID_SESSION => {
                            log::warn!(target: "qq", "服务端判定会话无效");
                            session.clear();
                            return Ok(ConnResult { outcome: Outcome::InvalidSession, ready });
                        }

                        OP_DISPATCH => {
                            let t = payload.t.unwrap_or_default();
                            if t == "READY" {
                                session.session_id = payload
                                    .d
                                    .get("session_id")
                                    .and_then(Value::as_str)
                                    .map(str::to_string);
                                ready = true;
                                self.set_status(ChannelStatus::Running);
                                log::info!(target: "qq", "QQ 机器人已就绪");
                            } else if t == "RESUMED" {
                                ready = true;
                                self.set_status(ChannelStatus::Running);
                                log::info!(target: "qq", "QQ 会话已恢复");
                            }

                            // READY / RESUMED 只用于维护会话，不交给业务层
                            let deliver = !t.is_empty() && t != "READY" && t != "RESUMED";
                            if deliver
                                && events
                                    .send(GatewayEvent {
                                        id: payload.id,
                                        t,
                                        d: payload.d,
                                    })
                                    .is_err()
                            {
                                // 消费端已退出，整条连接可以收尾
                                return Ok(ConnResult {
                                    outcome: Outcome::Closed,
                                    ready,
                                });
                            }
                        }

                        other => {
                            log::debug!(target: "qq", "未处理的网关 op={other}，已忽略");
                        }
                    }
                }
            }
        }
    }
}

async fn send_json(write: &mut WsSink, value: &Value) -> Result<(), ConnError> {
    write
        .send(Message::Text(value.to_string().into()))
        .await
        .map_err(|e| ConnError::Retryable(format!("发送 QQ 网关帧失败: {e}")))
}

/// Hello.d.heartbeat_interval，单位毫秒
fn heartbeat_from(d: &Value) -> u64 {
    let ms = match d.get("heartbeat_interval") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
    .unwrap_or(DEFAULT_HEARTBEAT_MS);

    ms.clamp(MIN_HEARTBEAT_MS, MAX_HEARTBEAT_MS)
}

/// 关闭码分类。4004 / 4013 / 4914 / 4915 属于配置或账号问题，重连没用
fn close_outcome(code: u16) -> Outcome {
    match code {
        4006..=4009 => Outcome::InvalidSession,
        _ => Outcome::Closed,
    }
}

fn close_is_fatal(code: u16) -> Option<&'static str> {
    match code {
        4004 => Some("AppID / AppSecret 或 access_token 无效（请检查 QQ 渠道配置）"),
        4013 => Some("QQ 机器人未订阅所需事件，或 intents 取值非法"),
        4014 => Some("QQ 机器人被限制了对应 intents 的权限"),
        4914 => Some("QQ 机器人已下线，请到 QQ 开放平台上线机器人"),
        4915 => Some("QQ 机器人已被封禁，停止连接"),
        _ => None,
    }
}

/// 退避：1s、2s、4s…… 封顶 30s，加一点抖动避免多实例同步重连
fn backoff(attempt: u32) -> Duration {
    let base = 1u64 << attempt.min(5);
    let capped = Duration::from_secs(base).min(MAX_BACKOFF);
    capped + Duration::from_millis((rand::random::<f64>() * 500.0) as u64)
}

/// REST 层错误里，凭证类问题直接判致命，其余按可重试处理
fn classify_api_error(err: anyhow::Error) -> ConnError {
    let msg = format!("{err:#}");
    if msg.contains("AppID")
        || msg.contains("AppSecret")
        || msg.contains("被拒绝")
        || msg.contains("HTTP 401")
        || msg.contains("HTTP 403")
    {
        ConnError::Fatal(msg)
    } else {
        ConnError::Retryable(msg)
    }
}

/// 只验证凭证与网关可用性，不建立长连接。供设置页「测试连接」使用
pub async fn test_credentials(api: &QqApi) -> Result<()> {
    api.get_gateway()
        .await
        .context("QQ 凭证校验未通过（获取网关地址失败）")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_interval_is_clamped() {
        assert_eq!(heartbeat_from(&json!({"heartbeat_interval": 41250})), 41250);
        assert_eq!(
            heartbeat_from(&json!({"heartbeat_interval": "41250"})),
            41250
        );
        assert_eq!(
            heartbeat_from(&json!({"heartbeat_interval": 0})),
            MIN_HEARTBEAT_MS
        );
        assert_eq!(heartbeat_from(&json!({})), DEFAULT_HEARTBEAT_MS);
    }

    #[test]
    fn close_codes_classified() {
        assert!(matches!(close_outcome(4006), Outcome::InvalidSession));
        assert!(matches!(close_outcome(1000), Outcome::Closed));
        assert!(close_is_fatal(4004).is_some());
        assert!(close_is_fatal(4915).is_some());
        assert!(close_is_fatal(4009).is_none());
    }

    #[test]
    fn backoff_is_bounded() {
        assert!(backoff(0) >= Duration::from_secs(1));
        assert!(backoff(20) <= MAX_BACKOFF + Duration::from_millis(500));
    }
}
