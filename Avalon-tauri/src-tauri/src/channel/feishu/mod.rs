// 飞书渠道：组装与运行
//
// 拼装顺序：token 提供者 → REST 封装 → 查机器人自身 open_id → 消息处理器 →
// 长连接。事件通过 mpsc 从长连接流向处理器，这条 channel 就是「立即 ACK、
// 异步处理」的分界线：左边只管收帧回执，右边慢慢跑 ReAct，互不阻塞。

pub mod api;
pub mod handler;
pub mod proto;
pub mod reaction;
pub mod stream;
pub mod token;
pub mod ws;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::channel::handle::{FeishuHandle, HandleGuard};
use crate::channel::ChannelStatus;
use crate::config::{ConfigStore, FeishuConfig};
use crate::engine::Engine;

pub use ws::test_credentials;

/// 渠道主入口：一直运行到被要求停止或遇到致命错误
pub async fn run(
    http: reqwest::Client,
    config: FeishuConfig,
    engine: Arc<Engine>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ChannelStatus>>,
    handle: Arc<FeishuHandle>,
    store: ConfigStore,
) {
    let base_url = config.base_url().to_string();

    let token = Arc::new(token::TokenProvider::new(
        http.clone(),
        &base_url,
        &config.app_id,
        &config.app_secret,
    ));
    let api = api::FeishuApi::new(http.clone(), &base_url, token);

    // 把 api 交给工具层，并挂一个 guard 保证本函数无论怎么结束都会清空 ——
    // 致命错误自然 return 的路上 ChannelManager::stop() 不会被调用，
    // 只靠那边的显式 clear 会漏
    handle.set(api.clone());
    let _guard = HandleGuard(handle);

    // 拿不到 open_id 不是致命问题：私聊照常工作，只是群聊的 @ 判定会失效。
    // 此时 mentioned_bot 恒为 false，群聊在默认配置下不会响应 —— 宁可不理，也好过乱理。
    let bot_open_id = match api.get_bot_open_id().await {
        Ok(id) => id,
        Err(e) => {
            log::warn!(target: "feishu", "获取机器人 open_id 失败，群聊 @ 判定将不可用: {e:#}");
            String::new()
        }
    };

    let handler = Arc::new(handler::MessageHandler::new(
        engine,
        api,
        config.clone(),
        bot_open_id,
        store,
    ));

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // 消费侧：每条消息独立 spawn，不同会话可以并发跑；
    // 同一会话内的串行由 handler 的 ChannelSlot 锁保证，这里不需要额外排队。
    let consumer = tauri::async_runtime::spawn(async move {
        while let Some(payload) = rx.recv().await {
            let handler = handler.clone();
            tauri::async_runtime::spawn(async move {
                handler.handle(payload).await;
            });
        }
    });

    let client = ws::WsClient::new(
        http,
        &base_url,
        &config.app_id,
        &config.app_secret,
        stop,
        status,
    );
    client.run(tx).await;

    // 长连接结束后 tx 已 drop，消费循环随之退出
    consumer.abort();
}
