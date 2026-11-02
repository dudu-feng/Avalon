// 渠道对接层
//
// 让 Engine 的能力不再局限于桌面窗口：外部渠道收到消息后，用与 scheduler
// 相同的方式驱动同一个 Engine。ChannelManager 只负责「每个渠道启停一个后台
// 任务」这一层，与具体协议无关；协议细节在 feishu/ 与 qq/ 里。
//
// 飞书与 QQ 各自维护独立的运行句柄与状态：一个挂了不影响另一个，
// 托盘与设置页也分别展示。

pub mod common;
pub mod contact;
pub mod feishu;
pub mod handle;
pub mod qq;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::channel::contact::ContactStore;
use crate::config::{FeishuConfig, QqConfig};
use crate::engine::Engine;

pub use feishu::api::Target;
pub use handle::{FeishuHandle, QqHandle};

/// 渠道标识，用于命令参数与前端展示
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Feishu,
    Qq,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Qq => "qq",
        }
    }
}

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

/// 单个渠道的可变状态
struct ChannelState {
    running: Mutex<Option<Running>>,
    status: Arc<Mutex<ChannelStatus>>,
}

impl ChannelState {
    fn new(status: ChannelStatus) -> Self {
        Self {
            running: Mutex::new(None),
            status: Arc::new(Mutex::new(status)),
        }
    }

    fn set_status(&self, s: ChannelStatus) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }

    fn status(&self) -> ChannelStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// 渠道生命周期管理器。放进 Tauri State，由托盘菜单与前端命令驱动
pub struct ChannelManager {
    feishu: ChannelState,
    qq: ChannelState,
    http: reqwest::Client,
    /// 工具层共享的飞书发送句柄。渠道跑起来时填入 api，停止时清空
    handle: Arc<FeishuHandle>,
    /// 工具层共享的 QQ 发送句柄，生命周期与飞书一致
    qq_handle: Arc<QqHandle>,
    /// 通讯录句柄，供 handler 自动记录主人
    contacts: Arc<ContactStore>,
}

impl ChannelManager {
    pub fn new(
        handle: Arc<FeishuHandle>,
        qq_handle: Arc<QqHandle>,
        contacts: Arc<ContactStore>,
    ) -> Self {
        // 长连接自己走 tokio-tungstenite，这个 client 只用于 REST 调用与凭证测试
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            feishu: ChannelState::new(ChannelStatus::Disabled),
            qq: ChannelState::new(ChannelStatus::Disabled),
            http,
            handle,
            qq_handle,
            contacts,
        }
    }

    fn state(&self, kind: ChannelKind) -> &ChannelState {
        match kind {
            ChannelKind::Feishu => &self.feishu,
            ChannelKind::Qq => &self.qq,
        }
    }

    pub fn status(&self, kind: ChannelKind) -> ChannelStatus {
        self.state(kind).status()
    }

    pub fn is_running(&self, kind: ChannelKind) -> bool {
        self.state(kind).status().is_active()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// 启动飞书渠道。已在运行时先停掉旧的（配置改动后重启即走这条路）
    pub fn start_feishu(&self, config: FeishuConfig, engine: Arc<Engine>) -> Result<(), String> {
        if !config.is_ready() {
            self.feishu.set_status(ChannelStatus::Disabled);
            return Err("飞书渠道未启用或 app_id / app_secret 未填写".to_string());
        }

        self.stop(ChannelKind::Feishu);

        let stop = Arc::new(AtomicBool::new(false));
        let status = self.feishu.status.clone();
        self.feishu.set_status(ChannelStatus::Connecting);

        let http = self.http.clone();
        let task_stop = stop.clone();
        let handle = self.handle.clone();
        let contacts = self.contacts.clone();
        let task = tauri::async_runtime::spawn(async move {
            feishu::run(http, config, engine, task_stop, status, handle, contacts).await;
        });

        *self
            .feishu
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Running { stop, task });
        Ok(())
    }

    /// 启动 QQ 渠道。同样先停旧的再起新的
    pub fn start_qq(&self, config: QqConfig, engine: Arc<Engine>) -> Result<(), String> {
        if !config.is_ready() {
            self.qq.set_status(ChannelStatus::Disabled);
            if config.enabled && config.backend == crate::config::QqBackend::Onebot {
                return Err("QQ 渠道的 OneBot 后端尚未实现，请选择 official".to_string());
            }
            return Err("QQ 渠道未启用或 AppID / AppSecret 未填写".to_string());
        }

        self.stop(ChannelKind::Qq);

        let stop = Arc::new(AtomicBool::new(false));
        let status = self.qq.status.clone();
        self.qq.set_status(ChannelStatus::Connecting);

        let task_stop = stop.clone();
        let contacts = self.contacts.clone();
        let qq_handle = self.qq_handle.clone();
        let task = tauri::async_runtime::spawn(async move {
            qq::run(config, engine, task_stop, status, contacts, qq_handle).await;
        });

        *self.qq.running.lock().unwrap_or_else(|e| e.into_inner()) = Some(Running { stop, task });
        Ok(())
    }

    /// 停止指定渠道。先置标志让循环自己收尾，再 abort 兜住卡在 await 上的情况
    pub fn stop(&self, kind: ChannelKind) {
        let state = self.state(kind);
        let Some(running) = state
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        running.stop.store(true, Ordering::Relaxed);
        running.task.abort();

        // abort 后 guard 何时跑取决于运行时调度，这里显式清空，
        // 避免停止后紧接着的工具调用仍以为渠道在线
        match kind {
            ChannelKind::Feishu => self.handle.clear(),
            ChannelKind::Qq => self.qq_handle.clear(),
        }
        state.set_status(ChannelStatus::Stopped);
    }

    /// 退出应用时停止全部渠道
    pub fn stop_all(&self) {
        self.stop(ChannelKind::Feishu);
        self.stop(ChannelKind::Qq);
    }
}
