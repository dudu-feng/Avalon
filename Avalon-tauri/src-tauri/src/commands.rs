// Tauri IPC 命令定义
//
// 前端通过 invoke("command_name", { params }) 调用这些命令。
// 所有命令均为 async，通过 Tauri State 共享配置和 LLM 客户端。

use std::sync::Mutex;
use tauri::State;

use crate::config::AppConfig;
use crate::llm::{LlmClient, LlmResponse};

// ============================================================
//  应用状态（通过 Tauri State 管理）
// ============================================================

/// 应用状态，在 Tauri 启动时初始化
pub struct AppState {
    /// 应用配置
    pub config: Mutex<AppConfig>,
    /// LLM 客户端
    pub llm_client: Mutex<Option<LlmClient>>,
}

// ============================================================
//  配置管理命令
// ============================================================

/// 获取当前应用配置
#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

/// 保存应用配置（前端修改后调用）
#[tauri::command]
pub fn save_config(
    mut new_config: AppConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // 继承原 .env 路径（前端不会传 env_path，它被 serde(skip) 了）
    let env_path = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        config.env_path.clone()
    };
    if new_config.env_path.as_os_str().is_empty() {
        new_config.env_path = env_path;
    }

    // 保存到 .env 文件
    new_config.save().map_err(|e| e.to_string())?;

    // 更新内存中的配置
    {
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        *config = new_config;
    }

    // 根据新配置重建 LLM 客户端
    {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        let mut llm_client = state.llm_client.lock().map_err(|e| e.to_string())?;
        match LlmClient::from_config(&config) {
            Ok(client) => *llm_client = Some(client),
            Err(e) => return Err(format!("重建 LLM 客户端失败: {}", e)),
        }
    }

    println!("[Config] 配置已更新并保存到 .env");
    Ok(())
}

/// 校验配置完整性，返回警告列表
#[tauri::command]
pub fn validate_config(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.validate())
}

/// 获取配置文件路径（现在是 .env 文件路径）
#[tauri::command]
pub fn get_config_path() -> Result<String, String> {
    AppConfig::env_file_path()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

// ============================================================
//  LLM 调用命令
// ============================================================

/// LLM 调用请求参数
#[derive(serde::Deserialize)]
pub struct ChatParams {
    /// 系统提示词
    pub system_prompt: String,
    /// 用户输入
    pub user_input: String,
    /// 聊天历史（JSON 字符串）
    pub chat_history: String,
}

/// 对话层 LLM 调用
#[tauri::command]
pub async fn llm_chat(
    params: ChatParams,
    state: State<'_, AppState>,
) -> Result<LlmResponse, String> {
    let client = {
        let llm_client = state.llm_client.lock().map_err(|e| e.to_string())?;
        llm_client
            .as_ref()
            .ok_or("LLM 客户端未初始化，请先配置 API Key")?
            .clone()
    };

    client
        .chat(&params.system_prompt, &params.user_input, &params.chat_history)
        .await
        .map_err(|e| e.to_string())
}

/// 动作层 LLM 调用请求参数
#[derive(serde::Deserialize)]
pub struct ActionParams {
    /// 原始用户输入
    pub user_input: String,
    /// 动作目标描述
    pub action_target: String,
    /// 已执行的操作历史（JSON 字符串）
    pub action_history: String,
}

/// 动作层 LLM 调用
#[tauri::command]
pub async fn llm_action(
    params: ActionParams,
    state: State<'_, AppState>,
) -> Result<LlmResponse, String> {
    let client = {
        let llm_client = state.llm_client.lock().map_err(|e| e.to_string())?;
        llm_client
            .as_ref()
            .ok_or("LLM 客户端未初始化，请先配置 API Key")?
            .clone()
    };

    client
        .action(
            &params.user_input,
            &params.action_target,
            &params.action_history,
        )
        .await
        .map_err(|e| e.to_string())
}

/// 会话压缩 LLM 调用
#[tauri::command]
pub async fn llm_compress(
    session_data: String,
    state: State<'_, AppState>,
) -> Result<LlmResponse, String> {
    let client = {
        let llm_client = state.llm_client.lock().map_err(|e| e.to_string())?;
        llm_client
            .as_ref()
            .ok_or("LLM 客户端未初始化，请先配置 API Key")?
            .clone()
    };

    client
        .compress(&session_data)
        .await
        .map_err(|e| e.to_string())
}

// ============================================================
//  系统命令
// ============================================================

/// 初始化应用（前端启动时调用）
///
/// 返回配置校验结果，前端据此判断是否需要引导用户配置 API Key
#[tauri::command]
pub fn init_app(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let warnings = config.validate();

    if warnings.is_empty() {
        println!("[App] 初始化完成，配置校验通过");
    } else {
        println!("[App] 初始化完成，配置有警告:");
        for w in &warnings {
            println!("  ⚠️ {}", w);
        }
    }

    Ok(warnings)
}

// ============================================================
//  命令注册宏
// ============================================================

/// 所有 Tauri 命令的注册宏，在 lib.rs 中展开
#[macro_export]
macro_rules! register_commands {
    () => {
        tauri::generate_handler![
            // 配置管理
            commands::get_config,
            commands::save_config,
            commands::validate_config,
            commands::get_config_path,
            // LLM 调用
            commands::llm_chat,
            commands::llm_action,
            commands::llm_compress,
            // 系统
            commands::init_app,
        ]
    };
}
