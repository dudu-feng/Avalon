// tenant_access_token 的获取与缓存
//
// 飞书的 token 有效期 2 小时。这里提前 5 分钟判过期，避免「刚检查还没到期、
// 请求发出去已经过期」的边界。并发拿 token 时可能重复请求一次，是幂等的，
// 不值得为此加异步锁把调用方串行化。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// 提前这么久就认为 token 该换了
const EXPIRY_MARGIN: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
struct TokenResp {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    tenant_access_token: String,
    /// 剩余有效秒数
    #[serde(default)]
    expire: u64,
}

struct Cached {
    token: String,
    expires_at: Instant,
}

pub struct TokenProvider {
    http: reqwest::Client,
    base_url: String,
    app_id: String,
    app_secret: String,
    cached: Mutex<Option<Cached>>,
}

impl TokenProvider {
    pub fn new(http: reqwest::Client, base_url: &str, app_id: &str, app_secret: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            cached: Mutex::new(None),
        }
    }

    /// 取一个可用 token，命中缓存就不发请求
    pub async fn get(&self) -> Result<String> {
        {
            // 单独作用域：MutexGuard 不能跨 await 存活
            let guard = self.cached.lock().unwrap();
            if let Some(c) = guard.as_ref() {
                if c.expires_at > Instant::now() {
                    return Ok(c.token.clone());
                }
            }
        }

        let (token, ttl) = self.fetch().await?;
        let expires_at = Instant::now() + ttl.saturating_sub(EXPIRY_MARGIN);
        *self.cached.lock().unwrap() = Some(Cached {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }

    /// 丢弃缓存。收到 99991663（token 失效）之类的响应时调用，下次自动重取
    pub fn invalidate(&self) {
        *self.cached.lock().unwrap() = None;
    }

    async fn fetch(&self) -> Result<(String, Duration)> {
        let url = format!("{}/open-apis/auth/v3/tenant_access_token/internal", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .context("请求 tenant_access_token 失败（网络不可达？）")?;

        let status = resp.status();
        let body = resp.text().await.context("读取 token 响应体失败")?;

        if !status.is_success() {
            bail!("获取 tenant_access_token 失败：HTTP {status}，响应 {body}");
        }

        let parsed: TokenResp =
            serde_json::from_str(&body).with_context(|| format!("解析 token 响应失败：{body}"))?;

        if parsed.code != 0 {
            bail!(
                "获取 tenant_access_token 被拒绝：code={} msg={}（请检查 app_id / app_secret）",
                parsed.code,
                parsed.msg
            );
        }
        if parsed.tenant_access_token.is_empty() {
            bail!("飞书返回了空 token：{body}");
        }

        // expire 理论上恒为 7200，兜底防止返回 0 导致每次都重取
        let ttl = Duration::from_secs(if parsed.expire == 0 { 7200 } else { parsed.expire });
        Ok((parsed.tenant_access_token, ttl))
    }
}
