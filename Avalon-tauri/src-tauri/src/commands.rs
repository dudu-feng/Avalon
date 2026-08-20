// Tauri IPC 命令定义
//
// 前端通过 invoke("command_name", { params }) 调用这些命令。
// 分两类：配置管理命令 + LLM 调用命令（LLM 命令由 engine 层后续编排）。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::config::{AppConfig, ConfigStore};
use crate::engine::{Engine, EngineEvent};
use crate::llm::{CompressResult, LlmState};
use crate::prompt::build_compress_prompt;
use crate::session::{ContextUsage, Message, SessionData, SessionMeta};
use crate::usage::{DailyUsageRow, UsageStore};
use crate::vector::{RebuildProgress, RebuildStats};

// ============ 配置管理 ============

/// 获取当前配置快照
#[tauri::command]
pub fn get_config(store: State<'_, ConfigStore>) -> Result<AppConfig, String> {
    Ok(store.get())
}

/// 切换活跃模型（校验存在后写回配置），返回校验警告列表
#[tauri::command]
pub fn set_active_model(name: String, store: State<'_, ConfigStore>) -> Result<Vec<String>, String> {
    store.set_active_model(&name).map_err(|e| e.to_string())
}

/// 保存配置（写回 Avalon-config.toml），返回校验警告列表
#[tauri::command]
pub fn save_config(
    mut new_config: AppConfig,
    store: State<'_, ConfigStore>,
) -> Result<Vec<String>, String> {
    // 前端不会传 config_path（serde(skip)），继承当前值
    let current = store.get();
    new_config.config_path = current.config_path;

    store.save(new_config).map_err(|e| e.to_string())
}

/// 校验当前配置，返回警告列表
#[tauri::command]
pub fn validate_config(store: State<'_, ConfigStore>) -> Result<Vec<String>, String> {
    Ok(store.validate())
}

/// 获取配置文件路径
#[tauri::command]
pub fn get_config_path(store: State<'_, ConfigStore>) -> Result<String, String> {
    Ok(store.get().config_path.to_string_lossy().to_string())
}

/// 初始化应用：返回配置校验结果（前端据此判断是否引导配置）
#[tauri::command]
pub fn init_app(store: State<'_, ConfigStore>) -> Result<Vec<String>, String> {
    let warnings = store.validate();
    for w in &warnings {
        println!("  ⚠️ {w}");
    }
    Ok(warnings)
}

// ============ LLM 调用 ============

/// 会话压缩：非流式 JSON，返回 summary + keywords。
#[tauri::command]
pub async fn llm_compress(
    session_data: String,
    config: State<'_, ConfigStore>,
    llm: State<'_, LlmState>,
) -> Result<CompressResult, String> {
    let cfg = config.get();
    let model = cfg.active_model_config().cloned()
        .ok_or_else(|| "未配置活跃模型（active_model 无效）".to_string())?;
    let client = llm.client(model, cfg.llm.clone());
    let (system, user) = build_compress_prompt(&session_data);
    client
        .compress(&system, &user)
        .await
        .map_err(|e| e.to_string())
}

// ============ Engine 编排 ============

/// 主聊天入口：跑完整 ReAct 双层循环，中间态经 Channel<EngineEvent> 逐事件推送。
/// 会话生命周期：调用方先 invoke("init_session")，chat 后 invoke("save_session")（决策 D3）。
#[tauri::command]
pub async fn chat(
    user_input: String,
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
    on_event: Channel<EngineEvent>,
) -> Result<(), String> {
    let cancel = engine.begin_chat(&channel_name);
    engine
        .run(&user_input, &channel_name, cancel, move |ev| {
            let _ = on_event.send(ev);
        })
        .await
        .map_err(|e| e.to_string())
}

/// 停止当前 channel 正在进行的流式生成（置位取消标志，chat 提前收尾返回部分结果）
#[tauri::command]
pub fn stop_chat(channel_name: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.stop_chat(&channel_name);
    Ok(())
}

/// 初始化会话（channel 维度）：active 复用 / 否则新建
#[tauri::command]
pub fn init_session(channel_name: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.init_session(&channel_name).map_err(|e| e.to_string())
}

/// 新建会话：归档当前（若非空），创建新的 active 会话，返回新会话完整数据
#[tauri::command]
pub async fn create_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    engine
        .create_session(&channel_name)
        .await
        .map_err(|e| e.to_string())
}

/// 读取当前会话完整数据（供前端加载历史 / 判断状态）
#[tauri::command]
pub fn get_current_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    engine.get_current_session(&channel_name).map_err(|e| e.to_string())
}

/// 读取当前会话上下文用量（最大输入 token vs 压缩阈值，供前端圆形进度条展示）
#[tauri::command]
pub fn get_context_usage(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<ContextUsage, String> {
    engine.get_context_usage(&channel_name).map_err(|e| e.to_string())
}

/// 归档当前会话（压缩 + 移入 history）
#[tauri::command]
pub async fn save_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    engine
        .save_session(&channel_name)
        .await
        .map_err(|e| e.to_string())
}

/// 列出全部会话元信息（active 置顶 + 归档按时间倒序），供会话历史列表
#[tauri::command]
pub fn list_sessions(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<SessionMeta>, String> {
    engine.list_sessions(&channel_name).map_err(|e| e.to_string())
}

/// 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据
#[tauri::command]
pub async fn switch_session(
    channel_name: String,
    id: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    engine
        .switch_session(&channel_name, &id)
        .await
        .map_err(|e| e.to_string())
}

/// 读取某会话最新压缩块的原始消息（供前端渲染归档历史）
#[tauri::command]
pub fn load_session_raw(
    id: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<Message>, String> {
    engine.load_session_raw(&id).map_err(|e| e.to_string())
}

/// 删除归档会话（目录 + 向量库该会话 chunk 一并清理）
#[tauri::command]
pub fn delete_session(id: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.delete_session(&id).map_err(|e| e.to_string())
}

/// 重命名会话标题（活跃或归档均可）
#[tauri::command]
pub fn rename_session(
    channel_name: String,
    id: String,
    title: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    engine
        .rename_session(&channel_name, &id, &title)
        .map_err(|e| e.to_string())
}

/// 重建会话向量库：清空 + 重扫 history/current + 重新入库（设置页维护操作）
/// 逐 session 处理时经 Channel<RebuildProgress> 上报进度。
/// 同步 CPU/IO 密集，用 spawn_blocking 避免阻塞主线程。
#[tauri::command]
pub async fn rebuild_memory_index(
    engine: State<'_, Arc<Engine>>,
    on_event: Channel<RebuildProgress>,
) -> Result<RebuildStats, String> {
    let engine = engine.inner().clone();
    tokio::task::spawn_blocking(move || {
        engine.rebuild_memory_index(move |p| {
            let _ = on_event.send(p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

// ============ 用量统计 ============

/// 查询最近 N 天用量（按「天 × 模型」展平），供首页控制台报表读取
#[tauri::command]
pub fn query_daily_usage(
    days: usize,
    usage: State<'_, Arc<UsageStore>>,
) -> Result<Vec<DailyUsageRow>, String> {
    Ok(usage.query_daily(days))
}
