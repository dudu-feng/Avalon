// QQ 渠道：组装与运行
//
// P0/P1 只实现 QQ 开放平台官方机器人。OneBot 11（NapCat 等）是非官方协议，
// 配置结构已预留但代码不实现 —— 不把有封号风险的路径做成默认可选项。
//
// 拼装顺序：token 提供者 → REST 封装 → 消息处理器 → WebSocket 网关。
// 事件通过 mpsc 从网关流向处理器，网关只管收帧和心跳，处理器异步跑 ReAct。

pub mod official;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::channel::contact::ContactStore;
use crate::channel::{ChannelStatus, QqHandle};
use crate::config::QqConfig;
use crate::engine::Engine;

use official::{MessageHandler, QqApi, TokenProvider};

/// 渠道主入口：一直运行到被要求停止或遇到致命错误
pub async fn run(
    config: QqConfig,
    engine: Arc<Engine>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ChannelStatus>>,
    contacts: Arc<ContactStore>,
    handle: Arc<QqHandle>,
) {
    if !config.is_ready() {
        log::warn!(target: "qq", "QQ 渠道未启用或凭证不全，拒绝启动");
        *status.lock().unwrap_or_else(|e| e.into_inner()) = ChannelStatus::Disabled;
        return;
    }

    let timeout = Duration::from_secs(config.official.timeout_secs.max(5));
    let http = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(c) => c,
        Err(e) => {
            let message = format!("创建 QQ HTTP 客户端失败: {e}");
            log::error!(target: "qq", "{message}");
            *status.lock().unwrap_or_else(|e| e.into_inner()) = ChannelStatus::Error { message };
            return;
        }
    };

    let token = Arc::new(TokenProvider::new(
        http.clone(),
        config.official.token_url(),
        &config.official.app_id,
        &config.official.app_secret,
    ));
    let api = QqApi::new(http, config.official.api_base(), token);

    // 工具层从这里开始能主动发消息；run 退出（正常停止、致命错误、task.abort）
    // 时 guard 析构并清空槽位，不会留下一个看起来在线的 api
    handle.set(api.clone());
    let _guard = crate::channel::handle::QqHandleGuard(handle);

    let handler = Arc::new(MessageHandler::new(
        engine,
        api.clone(),
        config.clone(),
        contacts,
    ));

    let (tx, mut rx) = mpsc::unbounded_channel();
    let consumer = tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let handler = handler.clone();
            // 每条事件独立跑，不同会话可并发；同一会话的串行由 SlotRegistry 保证
            tauri::async_runtime::spawn(async move {
                handler.handle(event).await;
            });
        }
    });

    let gateway = official::gateway::GatewayClient::new(
        api,
        config.official.effective_intents(),
        stop,
        status,
    );
    gateway.run(tx).await;

    consumer.abort();
}
