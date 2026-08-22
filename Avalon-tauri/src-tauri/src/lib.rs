// Avalon Tauri 应用入口
//
// 模块注册、配置加载、依赖链组装、Tauri Builder 配置。

mod commands;
mod config;
mod embedding;
mod engine;
mod llm;
mod prompt;
mod scheduler;
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
    let usage_store: Arc<usage::UsageStore> = Arc::new(usage::UsageStore::new(cfg.usage_path()));
    let task_store: Arc<scheduler::TaskStore> =
        Arc::new(scheduler::TaskStore::new(cfg.scheduler_path()));
    let tool_registry: Arc<dyn tool::ToolRegistry> = Arc::new(
        tool::ToolSet::new()
            .with_memory(memory_index)
            .with_config(store.clone())
            .with_scheduler(task_store.clone()),
    );
    let engine = Arc::new(engine::Engine::new(
        store.clone(),
        llm.clone(),
        prompt_asm,
        tool_registry,
        session_store,
        usage_store.clone(),
    ));

    // 供 setup 里启动心跳循环（manage 会 move 原句柄，这里提前 clone）
    let scheduler_engine = engine.clone();
    let scheduler_store = task_store.clone();

    // 4. 构建 Tauri 应用
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(store)
        .manage(llm)
        .manage(engine)
        .manage(usage_store)
        .manage(task_store)
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::set_active_model,
            commands::validate_config,
            commands::get_config_path,
            commands::init_app,
            commands::llm_compress,
            commands::chat,
            commands::stop_chat,
            commands::init_session,
            commands::create_session,
            commands::save_session,
            commands::get_current_session,
            commands::get_context_usage,
            commands::list_sessions,
            commands::switch_session,
            commands::load_session_history,
            commands::delete_session,
            commands::rename_session,
            commands::rebuild_memory_index,
            commands::query_daily_usage,
            commands::create_scheduled_task,
            commands::list_scheduled_tasks,
            commands::delete_scheduled_task,
            commands::toggle_scheduled_task,
            commands::mark_task_read,
            commands::get_unread_task_count,
        ])
        .setup(move |app| {
            // eager 预热：启动时后台加载，不阻塞主线程；失败降级（首次使用时 get_sync 再试）
            if cfg.embedding.load_mode == config::EmbeddingLoadMode::Eager {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = handle.warmup().await {
                        eprintln!("[Embedding] 预热失败: {e}");
                    }
                });
            }
            // 启动定时任务心跳循环（30s tick，静默执行 + 全局事件通知）
            let scheduler = scheduler::Scheduler::new(
                scheduler_engine,
                scheduler_store,
                app.handle().clone(),
            );
            tauri::async_runtime::spawn(async move {
                scheduler.run_loop(std::time::Duration::from_secs(30)).await;
            });
            println!("[App] Avalon Tauri 应用启动成功");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
