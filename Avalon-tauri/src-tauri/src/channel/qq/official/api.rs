// QQ 官方机器人 REST 封装
//
// 只覆盖 P0/P1 需要的能力：获取网关、发文本、C2C 流式帧、下载图片。
// 富消息（Markdown / 按钮 / 图文卡片）留到 P3，失败统一降级纯文本。
//
// QQ 的错误响应可能是 HTTP 200 + 业务 code，也可能直接非 2xx，两条路径都要检查。
// token 失效会清缓存重试一次，与飞书 api.rs 的处理方式一致。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use super::proto::{ChatType, Target};
use super::token::TokenProvider;

/// 单张图片下载上限，与飞书一致
const MAX_DOWNLOAD: u64 = 32 * 1024 * 1024;
/// 图片下载超时
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// QQ 的 token 失效错误码。不同版本用不同数字，宁多勿漏
fn is_token_error(status: reqwest::StatusCode, code: i64) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED || matches!(code, 11244 | 11245)
}

#[derive(Clone)]
pub struct QqApi {
    http: reqwest::Client,
    /// 与主 client 分开：图片下载必须禁止重定向，且 URL 来自外部事件
    download: reqwest::Client,
    base_url: String,
    token: Arc<TokenProvider>,
}

impl QqApi {
    pub fn new(http: reqwest::Client, base_url: &str, token: Arc<TokenProvider>) -> Self {
        let download = reqwest::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            // 附件 URL 带签名，重定向到非白名单主机会把请求带到不可信位置
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            http,
            download,
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    /// 取当前 access_token。WebSocket Identify/Resume 需要明文 token
    pub async fn token(&self) -> Result<String> {
        self.token.get().await
    }

    /// 获取 WebSocket 网关地址。
    ///
    /// 官方文档同时出现 `/gateway` 与 `/gateway/bot` 两种路径，不同账号权限下
    /// 可用的那个不一样：先试前者，失败再试后者。
    pub async fn get_gateway(&self) -> Result<String> {
        let first = self
            .request_raw(reqwest::Method::GET, "/gateway", None)
            .await;
        let raw = match first {
            Ok(v) if gateway_url(&v).is_some() => v,
            Ok(_) => self
                .request_raw(reqwest::Method::GET, "/gateway/bot", None)
                .await
                .context("QQ 网关接口未返回 url")?,
            Err(first_err) => self
                .request_raw(reqwest::Method::GET, "/gateway/bot", None)
                .await
                .with_context(|| format!("QQ 网关接口调用失败：{first_err:#}"))?,
        };

        gateway_url(&raw).context("QQ 网关接口未返回 url")
    }

    /// 发送一条消息，优先 Markdown，失败退回纯文本。
    ///
    /// 群聊与单聊的自定义 Markdown 已对所有机器人开放，不需要模板；但个别账号
    /// 仍可能被权限或内容校验拦住，退回纯文本比整条消息丢失好。
    /// 两种请求使用同一个 msg_id + msg_seq，服务端可按序号去重。
    pub async fn send_text(
        &self,
        target: &Target,
        text: &str,
        msg_id: &str,
        msg_seq: u32,
        quote_message_id: Option<&str>,
    ) -> Result<String> {
        match self
            .post_message(
                target,
                markdown_body(text, msg_id, msg_seq, quote_message_id),
            )
            .await
        {
            Ok(id) => Ok(id),
            Err(e) => {
                log::debug!(target: "qq", "Markdown 消息发送失败，退回纯文本: {e:#}");
                self.post_message(target, plain_body(text, msg_id, msg_seq, quote_message_id))
                    .await
            }
        }
    }

    async fn post_message(&self, target: &Target, body: Value) -> Result<String> {
        let data = self
            .request_raw(reqwest::Method::POST, &message_path(target), Some(&body))
            .await?;

        Ok(data
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// 发送一帧 C2C 流式消息。首帧返回 stream_msg_id；续帧/结束帧返回值可忽略
    pub async fn stream_c2c(&self, user_openid: &str, body: &Value) -> Result<Value> {
        self.request_raw(
            reqwest::Method::POST,
            &format!("/v2/users/{user_openid}/stream_messages"),
            Some(body),
        )
        .await
    }

    /// 下载 QQ 图片附件。
    ///
    /// URL 来自外部事件，按不可信输入处理：只允许 https、只允许腾讯媒体域名、
    /// 禁止重定向、限制大小。日志与错误信息里不出现带签名的完整 URL。
    pub async fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        let parsed = url::Url::parse(url).context("图片 URL 格式非法")?;
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();

        if parsed.scheme() != "https" {
            bail!("QQ 图片只允许 https，收到 {}://{host}", parsed.scheme());
        }
        if !is_allowed_media_host(&host) {
            bail!("QQ 图片域名不在白名单内: {host}");
        }

        let resp = self
            .download
            .get(parsed)
            .send()
            .await
            .with_context(|| format!("下载 QQ 图片失败: {host}"))?;

        let status = resp.status();
        if !status.is_success() {
            bail!("下载 QQ 图片失败: HTTP {status}（{host}）");
        }
        if resp.url().host_str().map(|h| h.to_ascii_lowercase()) != Some(host.clone()) {
            bail!("QQ 图片下载被重定向到非原主机，已拒绝");
        }
        if let Some(len) = resp.content_length() {
            if len > MAX_DOWNLOAD {
                bail!("QQ 图片超过上限 {} MB", MAX_DOWNLOAD / 1024 / 1024);
            }
        }

        let bytes = resp
            .bytes()
            .await
            .with_context(|| format!("读取 QQ 图片响应失败: {host}"))?;
        if bytes.is_empty() {
            bail!("QQ 返回空图片（{host}）");
        }
        if bytes.len() as u64 > MAX_DOWNLOAD {
            bail!("QQ 图片超过上限 {} MB", MAX_DOWNLOAD / 1024 / 1024);
        }
        Ok(bytes.to_vec())
    }

    async fn request_raw(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let mut last_err = String::new();

        // 最多两轮：第一轮失败若是 token 问题，刷新后再来一次
        for attempt in 0..2 {
            let token = self.token.get().await?;
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header("Authorization", format!("QQBot {token}"))
                .header("Content-Type", "application/json; charset=utf-8");
            if let Some(b) = body {
                req = req.json(b);
            }

            let resp = req
                .send()
                .await
                .with_context(|| format!("请求 QQ 接口失败: {path}"))?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .with_context(|| format!("读取 QQ 响应失败: {path}"))?;

            // 部分错误返回空 body（如 401），不能强制按 JSON 解析
            let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let code = parsed.get("code").and_then(Value::as_i64).unwrap_or(0);

            if status.is_success() && code == 0 {
                return Ok(parsed);
            }

            let msg = parsed
                .get("message")
                .or_else(|| parsed.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            last_err = format!("code={code} msg={msg} (HTTP {status})");

            if is_token_error(status, code) && attempt == 0 {
                self.token.invalidate();
                continue;
            }
            break;
        }

        bail!("QQ 接口 {path} 调用失败：{last_err}")
    }
}

fn gateway_url(raw: &Value) -> Option<String> {
    raw.get("url")
        .or_else(|| raw.pointer("/data/url"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// Markdown 消息体。填了 markdown 后顶层 content 必须为空
fn markdown_body(text: &str, msg_id: &str, msg_seq: u32, quote_message_id: Option<&str>) -> Value {
    let mut body = json!({
        "msg_type": 2,
        "markdown": { "content": text },
    });
    attach_reply(&mut body, msg_id, msg_seq, quote_message_id);
    body
}

fn plain_body(text: &str, msg_id: &str, msg_seq: u32, quote_message_id: Option<&str>) -> Value {
    let mut body = json!({
        "msg_type": 0,
        "content": text,
    });
    attach_reply(&mut body, msg_id, msg_seq, quote_message_id);
    body
}

/// 被动回复带 msg_id；主动消息 msg_id 为空。
///
/// 引用气泡是另一回事：`message_reference.message_id` 要的是入站事件
/// `message_scene.ext` 里的 `msg_idx`（REFIDX_ 开头），不是 message.id
/// （ROBOT1.0_ 开头）。调用方传什么就原样带上，平台不支持时由服务端拒绝。
fn attach_reply(body: &mut Value, msg_id: &str, msg_seq: u32, quote_message_id: Option<&str>) {
    if !msg_id.is_empty() {
        body["msg_id"] = Value::String(msg_id.to_string());
        body["msg_seq"] = Value::Number(msg_seq.into());
    }
    if let Some(quote) = quote_message_id.map(str::trim).filter(|s| !s.is_empty()) {
        body["message_reference"] = json!({ "message_id": quote });
    }
}

fn message_path(target: &Target) -> String {
    match target.chat_type {
        ChatType::Group => format!("/v2/groups/{}/messages", target.chat_id),
        ChatType::C2c => format!("/v2/users/{}/messages", target.chat_id),
    }
}

/// 腾讯媒体域名白名单。按后缀匹配，避免把 evi lqq.com.cn 之类的伪装域名放进来
fn is_allowed_media_host(host: &str) -> bool {
    const SUFFIXES: &[&str] = &["qpic.cn", "qq.com.cn", "qq.com"];
    SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_host_whitelist_matches_tencent_domains() {
        assert!(is_allowed_media_host("multimedia.nt.qq.com.cn"));
        assert!(is_allowed_media_host("gchat.qpic.cn"));
        assert!(is_allowed_media_host("qpic.cn"));
        assert!(is_allowed_media_host("example.qq.com"));
    }

    #[test]
    fn media_host_whitelist_rejects_lookalikes() {
        assert!(!is_allowed_media_host("evilqq.com.cn"));
        assert!(!is_allowed_media_host("qq.com.cn.evil.com"));
        assert!(!is_allowed_media_host("qpic.cn.evil.com"));
        assert!(!is_allowed_media_host("example.com"));
    }

    #[test]
    fn markdown_body_keeps_reply_anchor_and_has_no_top_level_content() {
        let body = markdown_body("**粗体**", "M1", 2, None);
        assert_eq!(body["msg_type"], 2);
        assert_eq!(body["markdown"]["content"], "**粗体**");
        assert_eq!(body["msg_id"], "M1");
        assert_eq!(body["msg_seq"], 2);
        // 官方要求：填了 markdown 后顶层 content 必须为空
        assert!(body.get("content").is_none());
    }

    #[test]
    fn plain_body_is_the_fallback_encoding() {
        let body = plain_body("**粗体**", "M1", 2, None);
        assert_eq!(body["msg_type"], 0);
        assert_eq!(body["content"], "**粗体**");
        assert_eq!(body["msg_id"], "M1");
        assert_eq!(body["msg_seq"], 2);
        assert!(body.get("markdown").is_none());
    }

    #[test]
    fn quote_adds_message_reference_without_touching_msg_id() {
        let body = markdown_body("hi", "", 0, Some("REFIDX_abc"));
        assert_eq!(body["message_reference"]["message_id"], "REFIDX_abc");
        // 主动消息没有 msg_id，引用不影响这个分支
        assert!(body.get("msg_id").is_none());

        let body = plain_body("hi", "M1", 1, Some("REFIDX_abc"));
        assert_eq!(body["msg_id"], "M1");
        assert_eq!(body["message_reference"]["message_id"], "REFIDX_abc");
    }

    #[test]
    fn empty_quote_is_ignored() {
        let body = plain_body("hi", "M1", 1, Some("   "));
        assert!(body.get("message_reference").is_none());
    }

    #[test]
    fn message_paths_follow_chat_type() {
        assert_eq!(
            message_path(&Target::new(ChatType::Group, "G1")),
            "/v2/groups/G1/messages"
        );
        assert_eq!(
            message_path(&Target::new(ChatType::C2c, "U1")),
            "/v2/users/U1/messages"
        );
    }
}
