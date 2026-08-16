// Tauri IPC 命令定义
//
// 前端通过 invoke("command_name", { params }) 调用这些命令。
// 当前仅承载配置管理命令，LLM 等业务命令由后续模块补充。

use tauri::State;

use crate::config::{AppConfig, ConfigStore};

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
