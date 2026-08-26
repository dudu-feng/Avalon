// 飞书长连接（WebSocket）客户端
//
// 协议流程逐条对照官方 Python SDK（larksuite/oapi-sdk-python 的 lark_oapi/ws/client.py）：
//   1. POST /callback/ws/endpoint 换取 wss 地址与 ClientConfig
//   2. 连上后按 PingInterval 发 CONTROL/ping 心跳
//   3. 收 DATA 帧 → sum>1 则按 message_id 重组 → 投递事件
//   4. 断开后按 ReconnectInterval 重连，首次带随机抖动
//
// 与官方 SDK 的一处关键差异：**我们收到帧就立刻 ACK，不等业务处理完**。
// 官方 SDK 是跑完业务逻辑才回 ACK，而 Avalon 的 ReAct 循环动辄几十秒，
// 必然超出飞书 3 秒的响应窗口并触发服务端重推同一条消息。
// 这里 ACK 与业务彻底解耦：帧一解析出来就回执，payload 丢进 channel 异步处理。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::proto::{self, Frame};
use crate::channel::ChannelStatus;

/// 分片缓存的存活时间，与官方 SDK 一致
const FRAGMENT_TTL: Duration = Duration::from_secs(5);

/// 服务端没下发配置时的兜底值
const DEFAULT_PING_INTERVAL: u64 = 120;
const DEFAULT_RECONNECT_INTERVAL: u64 = 120;
const DEFAULT_RECONNECT_NONCE: u64 = 30;

/// 事件 payload（JSON 原文），由 handler 侧解析
pub type EventSender = mpsc::UnboundedSender<Vec<u8>>;

/// 连接过程中的错误：区分「重试有意义」和「重试没意义」
enum ConnError {
    /// 凭证错误、权限不足、连接数超限 —— 再试多少次都一样，直接停
    Fatal(String),
    /// 网络抖动、服务端临时故障 —— 应当重连
    Retryable(String),
}

impl ConnError {
    fn message(&self) -> &str {
        match self {
            ConnError::Fatal(m) | ConnError::Retryable(m) => m,
        }
    }
}

/// 服务端下发的连接参数，握手和 pong 都会带
#[derive(Debug, Clone, Deserialize)]
struct ClientConfig {
    #[serde(rename = "ReconnectCount", default)]
    reconnect_count: i64,
    #[serde(rename = "ReconnectInterval", default)]
    reconnect_interval: u64,
    #[serde(rename = "ReconnectNonce", default)]
    reconnect_nonce: u64,
    #[serde(rename = "PingInterval", default)]
    ping_interval: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            reconnect_count: -1, // -1 = 无限重连
            reconnect_interval: DEFAULT_RECONNECT_INTERVAL,
            reconnect_nonce: DEFAULT_RECONNECT_NONCE,
            ping_interval: DEFAULT_PING_INTERVAL,
        }
    }
}

impl ClientConfig {
    /// 服务端偶尔会下发 0，全部兜底成默认值，否则会变成死循环或疯狂 ping
    fn sanitized(mut self) -> Self {
        if self.ping_interval == 0 {
            self.ping_interval = DEFAULT_PING_INTERVAL;
        }
        if self.reconnect_interval == 0 {
            self.reconnect_interval = DEFAULT_RECONNECT_INTERVAL;
        }
        self
    }
}

#[derive(Deserialize)]
struct EndpointData {
    #[serde(rename = "URL", default)]
    url: String,
    #[serde(rename = "ClientConfig")]
    client_config: Option<ClientConfig>,
}

#[derive(Deserialize)]
struct EndpointResp {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    msg: String,
    data: Option<EndpointData>,
}

/// 分片重组缓冲区
struct Fragments {
    buckets: HashMap<String, Bucket>,
}

struct Bucket {
    parts: Vec<Option<Vec<u8>>>,
    created: Instant,
}

impl Fragments {
    fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// 放入一个分片。集齐则返回拼好的完整 payload，否则返回 None
    fn push(&mut self, msg_id: &str, sum: usize, seq: usize, data: Vec<u8>) -> Option<Vec<u8>> {
        self.evict_expired();

        let bucket = self.buckets.entry(msg_id.to_string()).or_insert_with(|| Bucket {
            parts: vec![None; sum],
            created: Instant::now(),
        });

        // 服务端给的 seq 越界时直接丢弃这一片，避免 panic
        if seq >= bucket.parts.len() {
            eprintln!("[飞书] 分片序号越界: seq={seq} sum={sum} message_id={msg_id}");
            return None;
        }
        bucket.parts[seq] = Some(data);

        if bucket.parts.iter().any(Option::is_none) {
            return None;
        }

        let bucket = self.buckets.remove(msg_id)?;
        Some(bucket.parts.into_iter().flatten().flatten().collect())
    }

    fn evict_expired(&mut self) {
        let now = Instant::now();
        self.buckets
            .retain(|_, b| now.duration_since(b.created) < FRAGMENT_TTL);
    }
}

pub struct WsClient {
    http: reqwest::Client,
    base_url: String,
    app_id: String,
    app_secret: String,
    stop: Arc<AtomicBool>,
    status: Arc<std::sync::Mutex<ChannelStatus>>,
}

impl WsClient {
    pub fn new(
        http: reqwest::Client,
        base_url: &str,
        app_id: &str,
        app_secret: &str,
        stop: Arc<AtomicBool>,
        status: Arc<std::sync::Mutex<ChannelStatus>>,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            stop,
            status,
        }
    }

    fn set_status(&self, s: ChannelStatus) {
        *self.status.lock().unwrap() = s;
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// 主循环：连接 → 收发 → 断开 → 重连，直到被要求停止或遇到致命错误
    pub async fn run(&self, events: EventSender) {
        let mut cfg = ClientConfig::default();
        let mut attempt: i64 = 0;

        loop {
            if self.stopped() {
                break;
            }

            self.set_status(if attempt == 0 {
                ChannelStatus::Connecting
            } else {
                ChannelStatus::Reconnecting
            });

            match self.connect_once(&events, &mut cfg).await {
                Ok(()) => {
                    // 连接正常结束（服务端关闭），重连计数归零后继续
                    if self.stopped() {
                        break;
                    }
                    println!("[飞书] 连接已关闭，准备重连");
                    attempt = 0;
                }
                Err(ConnError::Fatal(message)) => {
                    eprintln!("[飞书] 致命错误，停止渠道: {message}");
                    self.set_status(ChannelStatus::Error { message });
                    return;
                }
                Err(e @ ConnError::Retryable(_)) => {
                    eprintln!("[飞书] 连接失败: {}", e.message());
                }
            }

            if self.stopped() {
                break;
            }

            // 重试次数用尽（reconnect_count 为 -1 时永不用尽）
            if cfg.reconnect_count >= 0 && attempt >= cfg.reconnect_count {
                let message = format!("重连 {} 次仍无法连接飞书服务器", cfg.reconnect_count);
                eprintln!("[飞书] {message}");
                self.set_status(ChannelStatus::Error { message });
                return;
            }

            // 首次重连加随机抖动，避免多实例同时重连打爆服务端
            let wait = if attempt == 0 && cfg.reconnect_nonce > 0 {
                Duration::from_secs_f64(rand::random::<f64>() * cfg.reconnect_nonce as f64)
            } else {
                Duration::from_secs(cfg.reconnect_interval)
            };
            tokio::time::sleep(wait).await;
            attempt += 1;
        }

        self.set_status(ChannelStatus::Stopped);
        println!("[飞书] 长连接已停止");
    }

    /// 建立一次连接并持续收发，直到断开
    async fn connect_once(
        &self,
        events: &EventSender,
        cfg: &mut ClientConfig,
    ) -> Result<(), ConnError> {
        let (url, negotiated) = self.negotiate().await?;
        if let Some(c) = negotiated {
            *cfg = c.sanitized();
        }

        // wss 地址的 query 里带着后续 ping 帧要用的 service_id
        let service_id = extract_query(&url, "service_id")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let device_id = extract_query(&url, "device_id").unwrap_or_default();

        let (stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .map_err(classify_handshake_error)?;

        println!("[飞书] 长连接已建立 (device_id={device_id})");
        self.set_status(ChannelStatus::Running);

        let (mut write, mut read) = stream.split();
        let mut fragments = Fragments::new();

        let mut ping_secs = cfg.ping_interval;
        let mut ticker = tokio::time::interval(Duration::from_secs(ping_secs));
        ticker.tick().await; // interval 首次立即就绪，先消费掉

        loop {
            if self.stopped() {
                let _ = write.close().await;
                return Ok(());
            }

            tokio::select! {
                incoming = read.next() => {
                    let Some(msg) = incoming else {
                        return Ok(()); // 流结束 = 连接关闭
                    };
                    let msg = msg.map_err(|e| ConnError::Retryable(format!("读取帧失败: {e}")))?;

                    let payload = match msg {
                        Message::Binary(b) => b,
                        Message::Close(_) => return Ok(()),
                        // 飞书只发二进制帧；ping/pong 由 tungstenite 自动处理
                        _ => continue,
                    };

                    let frame = match proto::decode_frame(&payload) {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("[飞书] 帧解码失败，已跳过: {e}");
                            continue;
                        }
                    };

                    match frame.method {
                        proto::FRAME_CONTROL => {
                            if let Some(new_cfg) = handle_control(&frame) {
                                *cfg = new_cfg.sanitized();
                                // 服务端调整了心跳间隔，重建 ticker
                                if cfg.ping_interval != ping_secs {
                                    ping_secs = cfg.ping_interval;
                                    ticker = tokio::time::interval(Duration::from_secs(ping_secs));
                                    ticker.tick().await;
                                }
                            }
                        }
                        proto::FRAME_DATA => {
                            // 先 ACK 再处理 —— 顺序不能反，见文件头注释
                            let ack = build_ack(&frame);
                            if let Err(e) = write.send(Message::Binary(ack.into())).await {
                                return Err(ConnError::Retryable(format!("回执发送失败: {e}")));
                            }

                            if let Some(data) = assemble(&frame, &mut fragments) {
                                if frame.header(proto::HEADER_TYPE) == Some(proto::MSG_EVENT) {
                                    // 投递失败说明 handler 侧已经关闭，整条连接可以收尾了
                                    if events.send(data).is_err() {
                                        return Ok(());
                                    }
                                }
                                // MSG_CARD（卡片回调）暂不处理：本期只做消息对话
                            }
                        }
                        other => {
                            eprintln!("[飞书] 未知帧类型 method={other}，已忽略");
                        }
                    }
                }

                _ = ticker.tick() => {
                    let ping = Frame::ping(service_id).to_bytes();
                    if let Err(e) = write.send(Message::Binary(ping.into())).await {
                        return Err(ConnError::Retryable(format!("心跳发送失败: {e}")));
                    }
                }
            }
        }
    }

    /// 端点协商：换取 wss 地址与连接参数
    async fn negotiate(&self) -> Result<(String, Option<ClientConfig>), ConnError> {
        let url = format!("{}/callback/ws/endpoint", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("locale", "zh")
            .json(&serde_json::json!({
                "AppID": self.app_id,
                "AppSecret": self.app_secret,
            }))
            .send()
            .await
            .map_err(|e| ConnError::Retryable(format!("端点协商请求失败: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ConnError::Retryable(format!("读取端点协商响应失败: {e}")))?;

        if !status.is_success() {
            return Err(ConnError::Retryable(format!(
                "端点协商返回 HTTP {status}: {body}"
            )));
        }

        let parsed: EndpointResp = serde_json::from_str(&body)
            .map_err(|e| ConnError::Retryable(format!("端点协商响应解析失败: {e} -> {body}")))?;

        match parsed.code {
            proto::CODE_OK => {}
            // 服务端忙 / 内部错误：可以重试
            proto::CODE_SYSTEM_BUSY => {
                return Err(ConnError::Retryable("飞书服务端繁忙".to_string()))
            }
            proto::CODE_INTERNAL_ERROR => {
                return Err(ConnError::Retryable(format!(
                    "飞书服务端内部错误: {}",
                    parsed.msg
                )))
            }
            // 其余都是配置/权限问题，重试无意义
            code => {
                return Err(ConnError::Fatal(format!(
                    "端点协商被拒绝 code={code} msg={}（请检查 app_id / app_secret，\
                     并确认这是一个开启了长连接的企业自建应用）",
                    parsed.msg
                )))
            }
        }

        let data = parsed
            .data
            .ok_or_else(|| ConnError::Retryable("端点协商响应缺少 data".to_string()))?;
        if data.url.is_empty() {
            return Err(ConnError::Retryable("端点协商未返回连接地址".to_string()));
        }

        Ok((data.url, data.client_config))
    }
}

/// 处理 CONTROL 帧。pong 会捎带新的连接参数
fn handle_control(frame: &Frame) -> Option<ClientConfig> {
    match frame.header(proto::HEADER_TYPE) {
        Some(proto::MSG_PONG) => {
            let payload = frame.payload.as_ref()?;
            if payload.is_empty() {
                return None;
            }
            serde_json::from_slice::<ClientConfig>(payload).ok()
        }
        // 服务端主动 ping 不需要回应（tungstenite 层已处理底层 ping/pong）
        _ => None,
    }
}

/// 组装 DATA 帧的 payload：非分片直接返回，分片则等集齐
fn assemble(frame: &Frame, fragments: &mut Fragments) -> Option<Vec<u8>> {
    let payload = frame.payload.clone().unwrap_or_default();
    let sum = frame.header_int(proto::HEADER_SUM, 1);

    if sum <= 1 {
        return Some(payload);
    }

    let msg_id = frame.header(proto::HEADER_MESSAGE_ID)?;
    let seq = frame.header_int(proto::HEADER_SEQ, 0);
    fragments.push(msg_id, sum as usize, seq.max(0) as usize, payload)
}

/// 构造回执帧：沿用原帧，只把 payload 换成成功响应
fn build_ack(frame: &Frame) -> Vec<u8> {
    let mut ack = frame.clone();
    // biz_rt 表示业务处理耗时。我们是立即回执、异步处理，如实填 0
    ack.set_header(proto::HEADER_BIZ_RT, "0");
    ack.payload = Some(br#"{"code":200,"headers":null,"data":null}"#.to_vec());
    ack.to_bytes()
}

/// 从 wss 地址的 query 中取参数
fn extract_query(url: &str, key: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// 握手失败时，真正的错误码藏在 HTTP 响应头里
fn classify_handshake_error(err: tokio_tungstenite::tungstenite::Error) -> ConnError {
    use tokio_tungstenite::tungstenite::Error as WsError;

    let WsError::Http(resp) = &err else {
        return ConnError::Retryable(format!("WebSocket 握手失败: {err}"));
    };

    let header = |name: &str| -> Option<String> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };

    let Some(code) = header(proto::HEADER_HANDSHAKE_STATUS).and_then(|v| v.parse::<i32>().ok())
    else {
        return ConnError::Retryable(format!("WebSocket 握手失败: {err}"));
    };
    let msg = header(proto::HEADER_HANDSHAKE_MSG).unwrap_or_else(|| "无描述".to_string());

    match code {
        proto::CODE_AUTH_FAILED => {
            let auth_code = header(proto::HEADER_HANDSHAKE_AUTH_ERRCODE)
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0);
            if auth_code == proto::CODE_EXCEED_CONN_LIMIT {
                ConnError::Fatal(format!(
                    "该飞书应用的长连接数已达上限（单应用最多 50 条）：{msg}。\
                     请确认没有其他机器或进程用同一个应用在线"
                ))
            } else {
                ConnError::Retryable(format!("握手鉴权失败 code={auth_code}: {msg}"))
            }
        }
        proto::CODE_FORBIDDEN => ConnError::Fatal(format!(
            "握手被拒绝（403）：{msg}。请确认应用已开启「长连接」事件订阅方式"
        )),
        other => ConnError::Retryable(format!("握手失败 status={other}: {msg}")),
    }
}

/// 只做一次端点协商用于验证凭证，不建立长连接。供设置页的「测试连接」使用
pub async fn test_credentials(
    http: &reqwest::Client,
    base_url: &str,
    app_id: &str,
    app_secret: &str,
) -> Result<()> {
    let client = WsClient::new(
        http.clone(),
        base_url,
        app_id,
        app_secret,
        Arc::new(AtomicBool::new(false)),
        Arc::new(std::sync::Mutex::new(ChannelStatus::Stopped)),
    );
    client
        .negotiate()
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{}", e.message()))
        .context("飞书凭证校验未通过")
}
