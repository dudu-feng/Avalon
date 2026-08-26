// 飞书开放平台 REST 封装
//
// 全部走普通 HTTP，用项目已有的 reqwest —— 这也是「不需要飞书 SDK」的底气所在：
// 真正私有的只有长连接那一段（见 ws.rs），发消息这边就是标准 JSON API。
//
// 飞书的响应统一是 {"code":0,"msg":"...","data":{...}}，HTTP 200 不代表业务成功，
// 必须查 code。token 过期（99991663 等）会自动清缓存重试一次。

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use super::token::TokenProvider;

/// token 失效类错误码，遇到就刷新重试
fn is_token_error(code: i64) -> bool {
    matches!(code, 99991661 | 99991663 | 99991664 | 99991668)
}

/// 取正文首行前若干字符作为卡片摘要，供会话列表预览
fn summarize(text: &str) -> String {
    const MAX: usize = 40;
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() > MAX {
        format!("{}…", line.chars().take(MAX).collect::<String>())
    } else {
        line.to_string()
    }
}

#[derive(Clone)]
pub struct FeishuApi {
    http: reqwest::Client,
    base_url: String,
    token: Arc<TokenProvider>,
}

impl FeishuApi {
    pub fn new(http: reqwest::Client, base_url: &str, token: Arc<TokenProvider>) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    /// 发送纯文本到会话，返回 message_id
    pub async fn send_text(&self, chat_id: &str, text: &str) -> Result<String> {
        // 飞书要求 content 本身是「JSON 的字符串形式」，不是嵌套对象 —— 双重编码
        let content = serde_json::to_string(&json!({ "text": text }))
            .context("序列化文本消息内容失败")?;

        let data = self
            .post(
                "/open-apis/im/v1/messages?receive_id_type=chat_id",
                &json!({
                    "receive_id": chat_id,
                    "msg_type": "text",
                    "content": content,
                }),
            )
            .await?;

        Ok(data
            .get("message_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// 以内联卡片发一段 markdown 正文，返回 message_id。
    ///
    /// 不走 CardKit（不需要 cardkit 权限），卡片 JSON 直接塞进消息 content。
    /// 之所以不用纯文本消息：模型输出必然带 markdown，而飞书 text 消息不渲染，
    /// `**粗体**`、列表、代码块都会原样露出来。
    /// summary 决定会话列表里的预览文案，不设的话那里只会显示「[卡片]」。
    pub async fn send_markdown(&self, chat_id: &str, text: &str) -> Result<String> {
        let card = json!({
            "schema": "2.0",
            "config": { "summary": { "content": summarize(text) } },
            "body": { "elements": [{ "tag": "markdown", "content": text }] },
        });
        let content = serde_json::to_string(&card).context("序列化正文卡片失败")?;

        let data = self
            .post(
                "/open-apis/im/v1/messages?receive_id_type=chat_id",
                &json!({
                    "receive_id": chat_id,
                    "msg_type": "interactive",
                    "content": content,
                }),
            )
            .await?;

        Ok(data
            .get("message_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// 给消息加一个表情回应，返回 reaction_id（取消时要用）
    pub async fn add_reaction(&self, message_id: &str, emoji_type: &str) -> Result<String> {
        let data = self
            .post(
                &format!("/open-apis/im/v1/messages/{message_id}/reactions"),
                &json!({ "reaction_type": { "emoji_type": emoji_type } }),
            )
            .await?;

        Ok(data
            .get("reaction_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// 撤掉之前加的表情回应
    pub async fn remove_reaction(&self, message_id: &str, reaction_id: &str) -> Result<()> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/open-apis/im/v1/messages/{message_id}/reactions/{reaction_id}"),
            None,
        )
        .await?;
        Ok(())
    }

    /// 全量替换卡片实体的内容（流式跑完后换成折叠面板用）
    pub async fn update_card(&self, card_id: &str, card: &Value, sequence: u32) -> Result<()> {
        let data = serde_json::to_string(card).context("序列化卡片结构失败")?;

        self.request(
            reqwest::Method::PUT,
            &format!("/open-apis/cardkit/v1/cards/{card_id}"),
            Some(&json!({
                "card": { "type": "card_json", "data": data },
                "sequence": sequence,
            })),
        )
        .await?;
        Ok(())
    }

    /// 查询机器人自身的 open_id —— 群聊里判断「有没有 @ 我」全靠它
    pub async fn get_bot_open_id(&self) -> Result<String> {
        // 这个接口的结果挂在顶层 bot 字段上，不在 data 里，故走 raw
        let raw = self
            .request_raw(reqwest::Method::GET, "/open-apis/bot/v3/info", None)
            .await?;

        raw.get("bot")
            .and_then(|b| b.get("open_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .context("飞书未返回机器人 open_id")
    }

    /// 创建一张预分配卡片实体，返回 card_id。
    ///
    /// CardKit 的流式能力建立在「先建实体、再发引用」之上：卡片内容归 cardkit 管，
    /// 消息只持有一个 card_id，于是后续每次更新都不必重发消息。
    pub async fn create_card(&self, card: &Value) -> Result<String> {
        // data 与 im 消息的 content 一样是「JSON 的字符串形式」
        let data = serde_json::to_string(card).context("序列化卡片结构失败")?;

        let resp = self
            .post(
                "/open-apis/cardkit/v1/cards",
                &json!({ "type": "card_json", "data": data }),
            )
            .await?;

        resp.get("card_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .context("飞书未返回 card_id")
    }

    /// 把已创建的卡片实体发到会话，返回 message_id
    pub async fn send_card(&self, chat_id: &str, card_id: &str) -> Result<String> {
        let content = serde_json::to_string(&json!({
            "type": "card",
            "data": { "card_id": card_id },
        }))
        .context("序列化卡片消息内容失败")?;

        let data = self
            .post(
                "/open-apis/im/v1/messages?receive_id_type=chat_id",
                &json!({
                    "receive_id": chat_id,
                    "msg_type": "interactive",
                    "content": content,
                }),
            )
            .await?;

        Ok(data
            .get("message_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// 流式更新卡片中某个元素的内容（覆盖语义，传的是累积全文而非增量）。
    ///
    /// sequence 必须严格递增 —— 飞书据此丢弃乱序到达的旧内容，
    /// 否则网络抖动会让卡片上的文字凭空「倒退」。
    pub async fn update_card_element(
        &self,
        card_id: &str,
        element_id: &str,
        content: &str,
        sequence: u32,
    ) -> Result<()> {
        // 注意是 PUT 而非 POST —— CardKit 这几个端点的动词并不统一
        self.request(
            reqwest::Method::PUT,
            &format!("/open-apis/cardkit/v1/cards/{card_id}/elements/{element_id}/content"),
            Some(&json!({ "content": content, "sequence": sequence })),
        )
        .await?;
        Ok(())
    }

    /// 关闭流式态。不调用的话卡片会一直显示加载动画，看起来像永远没跑完。
    pub async fn finish_card_streaming(&self, card_id: &str, sequence: u32) -> Result<()> {
        let settings = serde_json::to_string(&json!({
            "config": { "streaming_mode": false },
        }))
        .context("序列化卡片配置失败")?;

        // 这个端点是 PATCH
        self.request(
            reqwest::Method::PATCH,
            &format!("/open-apis/cardkit/v1/cards/{card_id}/settings"),
            Some(&json!({ "settings": settings, "sequence": sequence })),
        )
        .await?;
        Ok(())
    }

    /// POST 一个 JSON 请求，返回 data 字段
    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }

    /// 返回响应的 data 字段
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        let raw = self.request_raw(method, path, body).await?;
        Ok(raw.get("data").cloned().unwrap_or(Value::Null))
    }

    /// 返回完整响应体（少数接口把结果挂在 data 之外）
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
                .bearer_auth(&token)
                .header("Content-Type", "application/json; charset=utf-8");
            if let Some(b) = body {
                req = req.json(b);
            }

            let resp = req
                .send()
                .await
                .with_context(|| format!("请求飞书接口失败: {path}"))?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .with_context(|| format!("读取飞书响应失败: {path}"))?;

            let parsed: Value = serde_json::from_str(&text)
                .with_context(|| format!("飞书响应不是合法 JSON: {path} -> {text}"))?;
            let code = parsed.get("code").and_then(Value::as_i64).unwrap_or(-1);

            if code == 0 {
                return Ok(parsed);
            }

            let msg = parsed
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            last_err = format!("code={code} msg={msg} (HTTP {status})");

            if is_token_error(code) && attempt == 0 {
                self.token.invalidate();
                continue;
            }
            break;
        }

        bail!("飞书接口 {path} 调用失败：{last_err}")
    }
}
