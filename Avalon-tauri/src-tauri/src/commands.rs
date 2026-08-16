// Tauri IPC 命令定义
//
// 前端通过 invoke("command_name", { params }) 调用这些命令。
// 分两类：配置管理命令 + LLM 调用命令（LLM 命令由 engine 层后续编排）。

use tauri::ipc::Channel;
use tauri::State;

use crate::config::{AppConfig, ConfigStore};
use crate::llm::{ActionResult, ChatResult, CompressResult, LlmState, StreamEvent};
use crate::prompt::{build_action_prompt, build_compress_prompt};

// ============ 配置管理 ============

/// 获取当前配置快照
#[tauri::command]
pub fn get_config(store: State<'_, ConfigStore>) -> Result<AppConfig, String> {
    Ok(store.get())
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

/// 对话层：流式调用。正文/思考通过 on_event 通道逐字推送，返回完整结果。
#[tauri::command]
pub async fn llm_chat(
    system_prompt: String,
    user_input: String,
    chat_history: String,
    config: State<'_, ConfigStore>,
    llm: State<'_, LlmState>,
    on_event: Channel<StreamEvent>,
) -> Result<ChatResult, String> {
    let client = llm.client(config.get().llm);
    client
        .chat_stream(&system_prompt, &user_input, &chat_history, move |ev| {
            let _ = on_event.send(ev);
        })
        .await
        .map_err(|e| e.to_string())
}

/// 动作层：非流式 JSON，返回工具调用/子分析/完成意图。
#[tauri::command]
pub async fn llm_action(
    action_target: String,
    action_history: String,
    tool_list: Option<String>,
    config: State<'_, ConfigStore>,
    llm: State<'_, LlmState>,
) -> Result<ActionResult, String> {
    let client = llm.client(config.get().llm);
    let prompt = build_action_prompt(&action_target, &tool_list.unwrap_or_default(), &action_history);
    client
        .action(&prompt)
        .await
        .map_err(|e| e.to_string())
}

/// 会话压缩：非流式 JSON，返回 summary + keywords。
#[tauri::command]
pub async fn llm_compress(
    session_data: String,
    config: State<'_, ConfigStore>,
    llm: State<'_, LlmState>,
) -> Result<CompressResult, String> {
    let client = llm.client(config.get().llm);
    let (system, user) = build_compress_prompt(&session_data);
    client
        .compress(&system, &user)
        .await
        .map_err(|e| e.to_string())
}
