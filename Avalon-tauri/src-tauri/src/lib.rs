// Avalon Tauri 应用入口
//
// 模块注册、配置加载、Tauri Builder 配置。

mod commands;
mod config;
mod embedding;
mod llm;
mod tool;

#[cfg(test)]
mod test_file; // 单元测试（仅 cargo test 编译）

use config::ConfigStore;
use llm::LlmState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 1. 加载配置（失败时用内存默认配置兜底，避免应用无法启动）
    let store = match ConfigStore::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Config] 加载配置失败: {e}");
            eprintln!("[Config] 使用内存默认配置兜底");
            ConfigStore::from_config(config::default_config())
        }
    };

    // 2. 打印配置校验结果
    for w in store.validate() {
        println!("  ⚠️ {w}");
    }

    // 3. 构建 Tauri 应用
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(store)
        .manage(LlmState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::validate_config,
            commands::get_config_path,
            commands::init_app,
            commands::llm_chat,
            commands::llm_action,
            commands::llm_compress,
        ])
        .setup(|_app| {
            println!("[App] Avalon Tauri 应用启动成功");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
