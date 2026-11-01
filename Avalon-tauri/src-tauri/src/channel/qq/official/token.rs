// QQ 官方机器人 access_token
//
// 与飞书 tenant_access_token 不同，QQ 的 token 端点独立于 API 域名：
//   POST https://bots.qq.com/app/getAppAccessToken
//   body { "appId": "...", "clientSecret": "..." }
// 成功返回 access_token + expires_in（可能是数字，也可能是字符串 "7200"）。
//
// 有效期约 2 小时。提前 2 分钟判过期，避免请求发出时刚好过期。并发拿 token
// 可能重复请求一次，但接口幂等，不值得为此把调用方串行化。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// 提前这么久就认为 token 该换了
const EXPIRY_MARGIN: Duration = Duration::from_secs(120);
/// 服务端未返回 expires_in 时的兜底
const DEFAULT_TTL_SECS: u64 = 7200;

struct Cached {
    token: String,
    expires_at: Instant,
}

pub struct TokenProvider {
    http: reqwest::Client,
    token_url: String,
    app_id: String,
    app_secret: String,
    cached: Mutex<Option<Cached>>,
}

impl TokenProvider {
    pub fn new(http: reqwest::Client, token_url: &str, app_id: &str, app_secret: &str) -> Self {
        Self {
            http,
            token_url: token_url.to_string(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            cached: Mutex::new(None),
        }
    }

    /// 取一个可用 token，命中缓存就不发请求
    pub async fn get(&self) -> Result<String> {
        {
            // 单独作用域：MutexGuard 不能跨 await 存活
            let guard = self.cached.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = guard.as_ref() {
                if c.expires_at > Instant::now() {
                    return Ok(c.token.clone());
                }
            }
        }

        let (token, ttl) = self.fetch().await?;
        let expires_at = Instant::now() + ttl.saturating_sub(EXPIRY_MARGIN);
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = Some(Cached {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }

    /// 丢弃缓存。收到 token 失效响应时调用，下次自动重取
    pub fn invalidate(&self) {
        *self.cached.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    async fn fetch(&self) -> Result<(String, Duration)> {
        let resp = self
            .http
            .post(&self.token_url)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&serde_json::json!({
                "appId": self.app_id,
                "clientSecret": self.app_secret,
            }))
            .send()
            .await
            .context("请求 QQ access_token 失败（网络不可达？）")?;

        let status = resp.status();
        let body = resp.text().await.context("读取 QQ token 响应体失败")?;
        let parsed: Value = serde_json::from_str(&body)
            .with_context(|| format!("解析 QQ token 响应失败：{body}"))?;

        if !status.is_success() {
            bail!("获取 QQ access_token 失败：HTTP {status}，响应 {body}");
        }

        // 平台可能返回 HTTP 200 + 错误码，必须查业务码
        if let Some(code) = parsed.get("code").and_then(Value::as_i64) {
            if code != 0 {
                let msg = parsed
                    .get("message")
                    .or_else(|| parsed.get("msg"))
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                bail!(
                    "获取 QQ access_token 被拒绝：code={code} msg={msg}（请检查 AppID / AppSecret）"
                );
            }
        }

        let token = parsed
            .get("access_token")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if token.is_empty() {
            bail!("QQ 返回了空 access_token：{body}");
        }

        let ttl = Duration::from_secs(parse_expires_in(parsed.get("expires_in")));
        Ok((token.to_string(), ttl))
    }
}

/// expires_in 兼容数字与字符串；缺失或非法时退回默认 2 小时
fn parse_expires_in(value: Option<&Value>) -> u64 {
    let secs = match value {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
    .unwrap_or(DEFAULT_TTL_SECS);

    if secs == 0 {
        DEFAULT_TTL_SECS
    } else {
        secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_in_accepts_number_and_string() {
        assert_eq!(parse_expires_in(Some(&serde_json::json!(7200))), 7200);
        assert_eq!(parse_expires_in(Some(&serde_json::json!("7200"))), 7200);
        assert_eq!(
            parse_expires_in(Some(&serde_json::json!("not-a-number"))),
            DEFAULT_TTL_SECS
        );
        assert_eq!(parse_expires_in(None), DEFAULT_TTL_SECS);
        assert_eq!(
            parse_expires_in(Some(&serde_json::json!(0))),
            DEFAULT_TTL_SECS
        );
    }
}
