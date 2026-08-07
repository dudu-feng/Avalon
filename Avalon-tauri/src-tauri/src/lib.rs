// Avalon Tauri 应用入口
//
// 模块注册、状态初始化、Tauri Builder 配置

mod commands;
mod config;
mod llm;

use std::sync::Mutex;

use commands::AppState;
use config::AppConfig;
use llm::LlmClient;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 1. 加载配置（首次启动自动创建默认配置文件）
    let config = match AppConfig::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[Fatal] 加载配置失败: {}", e);
            // 使用默认配置启动，避免应用无法运行
            AppConfig::default_config()
        }
    };

    // 2. 初始化 LLM 客户端
    let llm_client = match LlmClient::from_config(&config) {
        Ok(client) => {
            println!("[App] LLM 客户端初始化成功");
            Some(client)
        }
        Err(e) => {
            eprintln!("[App] LLM 客户端初始化失败: {}", e);
            None
        }
    };

    // 3. 校验配置
    let warnings = config.validate();
    for w in &warnings {
        println!("  ⚠️ {}", w);
    }

    // 4. 构建 Tauri 应用
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            config: Mutex::new(config),
            llm_client: Mutex::new(llm_client),
        })
        .invoke_handler(register_commands!())
        .setup(|_app| {
            println!("[App] Avalon Tauri 应用启动成功");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
