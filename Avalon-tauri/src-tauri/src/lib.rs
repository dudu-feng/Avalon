// Avalon Tauri 应用入口
//
// 模块注册、配置加载、依赖链组装、Tauri Builder 配置。

mod commands;
mod config;
mod embedding;
mod engine;
mod llm;
mod prompt;
mod session;
mod tool;
mod usage;
mod vector;

#[cfg(test)]
mod test_file; // 单元测试（仅 cargo test 编译）

use std::sync::Arc;

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

    // 3. 组装依赖链（决策甲：EmbedderHandle 贯穿 embedding→vector→session，load_mode 由配置驱动）
    let cfg = store.get();
    let handle = embedding::EmbedderHandle::new(cfg.clone());
    let llm = LlmState::new();

    let (vector_store, memory_index) = match vector::build(&cfg, handle.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[Vector] 初始化向量库失败: {e}");
            eprintln!("[Vector] 应用无法启动（向量库为会话记忆核心依赖）");
            return;
        }
    };
    let session_store: Arc<dyn session::SessionStore> = Arc::new(session::FileSessionStore::new(
        store.clone(),
        llm.clone(),
        vector_store,
    ));
    let prompt_asm = prompt::PromptAssembler::new(&cfg);
    let tool_registry: Arc<dyn tool::ToolRegistry> =
        Arc::new(tool::ToolSet::new().with_memory(memory_index).with_config(store.clone()));
    let usage_store: Arc<usage::UsageStore> = Arc::new(usage::UsageStore::new(cfg.usage_path()));
    let engine = Arc::new(engine::Engine::new(
        store.clone(),
        llm.clone(),
        prompt_asm,
        tool_registry,
        session_store,
        usage_store.clone(),
    ));

    // 4. 构建 Tauri 应用
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(store)
        .manage(llm)
        .manage(engine)
        .manage(usage_store)
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::validate_config,
            commands::get_config_path,
            commands::init_app,
            commands::llm_compress,
            commands::chat,
            commands::init_session,
            commands::save_session,
            commands::get_current_session,
            commands::get_context_usage,
            commands::rebuild_memory_index,
            commands::query_daily_usage,
        ])
        .setup(move |_app| {
            // eager 预热：启动时后台加载，不阻塞主线程；失败降级（首次使用时 get_sync 再试）
            if cfg.embedding.load_mode == config::EmbeddingLoadMode::Eager {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = handle.warmup().await {
                        eprintln!("[Embedding] 预热失败: {e}");
                    }
                });
            }
            println!("[App] Avalon Tauri 应用启动成功");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
