// IPC：会话生命周期、压缩、历史加载与记忆索引重建。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::engine::Engine;
use crate::session::{ContextUsage, LoadHistoryResult, SessionData, SessionMeta};
use crate::vector::{RebuildProgress, RebuildStats};

/// 初始化会话：active 复用 / 否则新建 main
#[tauri::command]
pub fn init_session(engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.init_session().map_err(|e| e.to_string())
}

/// 新建会话：归档当前（若非空），创建新的 active 会话，返回新会话完整数据
#[tauri::command]
pub async fn create_session(engine: State<'_, Arc<Engine>>) -> Result<SessionData, String> {
    ensure_idle(&engine, "新建会话")?;
    engine.create_session().await.map_err(|e| e.to_string())
}

/// 读取当前会话完整数据（供前端加载历史 / 判断状态）
#[tauri::command]
pub fn get_current_session(engine: State<'_, Arc<Engine>>) -> Result<SessionData, String> {
    engine.get_current_session().map_err(|e| e.to_string())
}

/// 读取当前会话上下文用量（最大输入 token vs 压缩阈值，供前端圆形进度条展示）
#[tauri::command]
pub fn get_context_usage(engine: State<'_, Arc<Engine>>) -> Result<ContextUsage, String> {
    engine.get_context_usage().map_err(|e| e.to_string())
}

/// 归档当前会话（压缩 + 移入 history）
#[tauri::command]
pub async fn save_session(engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.save_session().await.map_err(|e| e.to_string())
}

/// 主动压缩当前会话上下文（不归档），返回是否实际压缩（空会话返回 false）
#[tauri::command]
pub async fn compress_session(engine: State<'_, Arc<Engine>>) -> Result<bool, String> {
    engine
        .compress_session()
        .await
        .map_err(|e| e.to_string())
}

/// 列出全部会话元信息（active 置顶 + 归档按时间倒序），供会话历史列表
#[tauri::command]
pub fn list_sessions(engine: State<'_, Arc<Engine>>) -> Result<Vec<SessionMeta>, String> {
    engine.list_sessions().map_err(|e| e.to_string())
}

/// 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据
#[tauri::command]
pub async fn switch_session(
    id: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    ensure_idle(&engine, "切换会话")?;
    engine
        .switch_session(&id)
        .await
        .map_err(|e| e.to_string())
}

/// 渐进式加载历史块：before_chunk=None 读最新块，否则读更早一块（供前端渐进式回溯会话历史）
#[tauri::command]
pub fn load_session_history(
    id: String,
    before_chunk: Option<u64>,
    engine: State<'_, Arc<Engine>>,
) -> Result<LoadHistoryResult, String> {
    engine
        .load_session_history(&id, before_chunk)
        .map_err(|e| e.to_string())
}

/// 删除归档会话（目录 + 向量库该会话 chunk 一并清理）
#[tauri::command]
pub fn delete_session(id: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    ensure_idle(&engine, "删除会话")?;
    engine.delete_session(&id).map_err(|e| e.to_string())
}

/// 重命名会话标题（活跃或归档均可）
#[tauri::command]
pub fn rename_session(
    id: String,
    title: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    // 重命名与 run 收尾都会对 main.json 做 read-modify-write，
    // 生成期间改名会让整轮对话被旧快照覆盖，必须等空闲。
    ensure_idle(&engine, "重命名会话")?;
    engine
        .rename_session(&id, &title)
        .map_err(|e| e.to_string())
}

/// 有执行中或排队中的消息时拒绝会话控制操作（后端最终校验）。
fn ensure_idle(engine: &Engine, action: &str) -> Result<(), String> {
    let pending = engine.pending_count();
    if pending > 0 {
        return Err(format!(
            "当前有 {pending} 条消息正在执行或排队，暂不能{action}"
        ));
    }
    Ok(())
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
