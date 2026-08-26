// 渠道对接层
//
// 让 Engine 的能力不再局限于桌面窗口：外部渠道（当前是飞书）收到消息后，
// 用与 scheduler 相同的方式驱动同一个 Engine —— 拿一个 channel 字符串跑 ReAct，
// 区别只在于事件回调要把结果发回渠道，而不是丢弃或推给前端。
//
// ChannelManager 只负责「启停一个后台任务」这一层，与具体协议无关。
// 暂不抽 ChannelAdapter trait：只有一个实现的 trait 是负债，等接第二个渠道时共性才看得清。

pub mod feishu;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::config::FeishuConfig;
use crate::engine::Engine;

/// 渠道运行状态，直接序列化给前端
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ChannelStatus {
    /// 未启用或凭证不全
    Disabled,
    /// 已停止
    Stopped,
    /// 正在建立连接
    Connecting,
    /// 连接正常，可收发消息
    Running,
    /// 断线重连中
    Reconnecting,
    /// 因致命错误停止（凭证错误、连接数超限等），不会自动恢复
    Error { message: String },
}

impl ChannelStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ChannelStatus::Connecting | ChannelStatus::Running | ChannelStatus::Reconnecting
        )
    }
}

/// 正在运行的渠道句柄
struct Running {
    stop: Arc<AtomicBool>,
    task: tauri::async_runtime::JoinHandle<()>,
}

/// 渠道生命周期管理器。放进 Tauri State，由托盘菜单与前端命令驱动
pub struct ChannelManager {
    running: Mutex<Option<Running>>,
    status: Arc<Mutex<ChannelStatus>>,
    http: reqwest::Client,
}

impl ChannelManager {
    pub fn new() -> Self {
        // 长连接自己走 tokio-tungstenite，这个 client 只用于 REST 调用
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            running: Mutex::new(None),
            status: Arc::new(Mutex::new(ChannelStatus::Disabled)),
            http,
        }
    }

    pub fn status(&self) -> ChannelStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// 启动飞书渠道。已在运行时先停掉旧的（配置改动后重启即走这条路）
    pub fn start(&self, config: FeishuConfig, engine: Arc<Engine>) -> Result<(), String> {
        if !config.is_ready() {
            *self.status.lock().unwrap() = ChannelStatus::Disabled;
            return Err("飞书渠道未启用或 app_id / app_secret 未填写".to_string());
        }

        self.stop();

        let stop = Arc::new(AtomicBool::new(false));
        let status = self.status.clone();
        *status.lock().unwrap() = ChannelStatus::Connecting;

        let http = self.http.clone();
        let task_stop = stop.clone();
        let task = tauri::async_runtime::spawn(async move {
            feishu::run(http, config, engine, task_stop, status).await;
        });

        *self.running.lock().unwrap() = Some(Running { stop, task });
        Ok(())
    }

    /// 停止渠道。先置标志让循环自己收尾，再 abort 兜住卡在 await 上的情况
    pub fn stop(&self) {
        let Some(running) = self.running.lock().unwrap().take() else {
            return;
        };
        running.stop.store(true, Ordering::Relaxed);
        running.task.abort();
        *self.status.lock().unwrap() = ChannelStatus::Stopped;
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}
