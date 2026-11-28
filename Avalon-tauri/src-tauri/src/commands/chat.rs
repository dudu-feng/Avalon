// IPC：主聊天入口与运行控制。

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::engine::{Engine, EngineEvent, RunContext};
use crate::vision::ChatAttachment;

/// 主聊天入口：跑完整 ReAct 双层循环，中间态经 Channel<EngineEvent> 逐事件推送。
/// 所有来源写入同一个 main 会话；会话生命周期由前端按需调用 init/save。
#[tauri::command]
pub async fn chat(
    user_input: String,
    attachments: Option<Vec<ChatAttachment>>,
    run_id: Option<String>,
    engine: State<'_, Arc<Engine>>,
    on_event: Channel<EngineEvent>,
) -> Result<(), String> {
    let run = match run_id {
        Some(id) if !id.trim().is_empty() => RunContext::desktop(id),
        _ => RunContext::desktop(Engine::new_run_id("desktop")),
    };
    let input = crate::engine::UserInput {
        for_model: &user_input,
        for_history: &user_input,
        meta: Some(run.origin.clone()),
        attachments: attachments.unwrap_or_default(),
    };
    engine
        .run_with_input(input, run, move |ev| {
            let _ = on_event.send(ev);
        })
        .await
        .map_err(|e| e.to_string())
}

/// 停止指定 run 的流式生成（置位取消标志，chat 提前收尾返回部分结果）
#[tauri::command]
pub fn stop_chat(run_id: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.stop_chat(&run_id);
    Ok(())
}

/// 当前执行中 / 排队中的 run（前端据此显示处理中各来源与禁用会话操作）
#[derive(serde::Serialize)]
pub struct ActiveRuns {
    pub pending: usize,
    pub runs: Vec<RunContext>,
}

#[tauri::command]
pub fn active_runs(engine: State<'_, Arc<Engine>>) -> Result<ActiveRuns, String> {
    Ok(ActiveRuns {
        pending: engine.pending_count(),
        runs: engine.active_runs(),
    })
}
