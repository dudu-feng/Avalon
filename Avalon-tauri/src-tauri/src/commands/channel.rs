// IPC：飞书 / QQ 渠道控制与凭证测试。

use std::sync::Arc;

use tauri::State;

use crate::channel::{ChannelKind, ChannelManager, ChannelStatus};
use crate::config::ConfigStore;
use crate::engine::Engine;

/// 启动飞书渠道。读取最新配置，已在运行则先停后启（改完配置直接调它即可生效）
#[tauri::command]
pub fn feishu_start(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    channels.start_feishu(config.get().feishu, engine.inner().clone())
}

/// 停止飞书渠道
#[tauri::command]
pub fn feishu_stop(channels: State<'_, Arc<ChannelManager>>) -> Result<(), String> {
    channels.stop(ChannelKind::Feishu);
    Ok(())
}

/// 查询飞书渠道当前状态
#[tauri::command]
pub fn feishu_status(channels: State<'_, Arc<ChannelManager>>) -> Result<ChannelStatus, String> {
    Ok(channels.status(ChannelKind::Feishu))
}

/// 启动 QQ 渠道。读取最新配置，已在运行则先停后启
#[tauri::command]
pub fn qq_start(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    channels.start_qq(config.get().qq, engine.inner().clone())
}

/// 停止 QQ 渠道
#[tauri::command]
pub fn qq_stop(channels: State<'_, Arc<ChannelManager>>) -> Result<(), String> {
    channels.stop(ChannelKind::Qq);
    Ok(())
}

/// 查询 QQ 渠道当前状态
#[tauri::command]
pub fn qq_status(channels: State<'_, Arc<ChannelManager>>) -> Result<ChannelStatus, String> {
    Ok(channels.status(ChannelKind::Qq))
}

/// 测试 QQ 凭证：取一次 token 和网关地址，不建立长连接
#[tauri::command]
pub async fn qq_test_connection(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
) -> Result<(), String> {
    let cfg = config.get().qq;
    if cfg.official.app_id.is_empty() || cfg.official.app_secret.is_empty() {
        return Err("请先填写 AppID 与 AppSecret".to_string());
    }

    let token = Arc::new(crate::channel::qq::official::TokenProvider::new(
        channels.http().clone(),
        cfg.official.token_url(),
        &cfg.official.app_id,
        &cfg.official.app_secret,
    ));
    let api = crate::channel::qq::official::QqApi::new(
        channels.http().clone(),
        cfg.official.api_base(),
        token,
    );

    crate::channel::qq::official::test_credentials(&api)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// 测试凭证：只做一次端点协商，不建立长连接，也不影响正在运行的渠道
#[tauri::command]
pub async fn feishu_test_connection(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
) -> Result<(), String> {
    let cfg = config.get().feishu;
    if cfg.app_id.is_empty() || cfg.app_secret.is_empty() {
        return Err("请先填写 app_id 与 app_secret".to_string());
    }

    crate::channel::feishu::test_credentials(
        channels.http(),
        cfg.base_url(),
        &cfg.app_id,
        &cfg.app_secret,
    )
    .await
    .map_err(|e| format!("{e:#}"))
}
