// Avalon Tauri 应用入口
//
// 模块注册、配置加载、依赖链组装、Tauri Builder 配置。

mod channel;
mod commands;
mod config;
mod embedding;
mod engine;
mod llm;
mod logging;
mod prompt;
mod scheduler;
mod session;
mod tool;
mod tray;
mod usage;
mod vector;

#[cfg(test)]
mod test_file; // 单元测试（仅 cargo test 编译）

use std::sync::Arc;

use config::ConfigStore;
use llm::LlmState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志插件要等 Tauri Builder 起来才接管 log 宏，而配置加载发生在那之前，
    // 此刻打的日志会被直接丢弃。先攒着，setup 里补记 ——
    // 否则打包后配置读不出来时，日志里一点痕迹都没有
    let mut boot_log: Vec<(log::Level, String)> = Vec::new();

    // 1. 加载配置（失败时用内存默认配置兜底，避免应用无法启动）
    let store = match ConfigStore::load() {
        Ok(s) => s,
        Err(e) => {
            boot_log.push((log::Level::Error, format!("加载配置失败: {e}")));
            boot_log.push((log::Level::Warn, "已使用内存默认配置兜底".to_string()));
            ConfigStore::from_config(config::default_config())
        }
    };

    // 2. 建默认工作区，再校验。
    //    顺序不能反：validate 会检查工作区根是否存在，而首次启动时它还没被创建，
    //    先校验的话每台新机器第一次开都会报一条假警告。
    //    只建默认那个 —— 用户显式配的路径由用户自己保证存在，替他创建
    //    等于把一个笔误的路径变成一个真目录，反而更难发现
    if store.get().tools.workspace_roots.is_none() {
        let ws = store.get().workspace_path();
        if let Err(e) = std::fs::create_dir_all(&ws) {
            boot_log.push((
                log::Level::Warn,
                format!("创建工作区目录失败（文件工具将不可用）: {} - {e}", ws.display()),
            ));
        }
    }

    for w in store.validate() {
        boot_log.push((log::Level::Warn, format!("配置校验: {w}")));
    }

    // 3. 组装依赖链（决策甲：EmbedderHandle 贯穿 embedding→vector→session，load_mode 由配置驱动）
    let cfg = store.get();
    let handle = embedding::EmbedderHandle::new(cfg.clone());
    let llm = LlmState::new(cfg.llm.timeout_secs);

    let (vector_store, memory_index) = match vector::build(&cfg, handle.clone()) {
        Ok(v) => v,
        Err(e) => {
            // 这里直接 return，Builder 不会运行，攒着的日志也就没机会补记 ——
            // 只能靠 stderr，dev 下看得到，打包后表现为「点了图标没反应」
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
    // 飞书发送句柄。必须先于 ToolSet 创建 —— 工具层拿的是这个 Arc，
    // 渠道启动时才往里填 api。空句柄下工具会如实回答「渠道未运行」
    let feishu_handle = Arc::new(channel::FeishuHandle::new());
    let mut tool_set = tool::ToolSet::new()
        .with_memory(memory_index)
        .with_config(store.clone())
        .with_scheduler(task_store.clone())
        .with_feishu(feishu_handle.clone());
    // 搜索工具按配置开关注入：不注入就等于对模型完全隐藏，
    // 比注入之后再在调用时拒绝要干净 —— 模型不会反复尝试一个用不了的工具
    if cfg.search.enabled {
        tool_set = tool_set.with_search(tool::web_tools::SearchClient::new(cfg.search.clone()));
        boot_log.push((log::Level::Info, "联网搜索工具已启用".to_string()));
    }
    let tool_registry: Arc<dyn tool::ToolRegistry> = Arc::new(tool_set);
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

    // 渠道对接层：与 scheduler 一样，拿 engine 句柄在后台驱动 ReAct
    let channels = Arc::new(channel::ChannelManager::new(feishu_handle, store.clone()));
    let channel_engine = engine.clone();
    let channel_manager = channels.clone();
    let feishu_cfg = cfg.feishu.clone();

    // 4. 构建 Tauri 应用
    tauri::Builder::default()
        // 单实例必须第一个注册：插件按添加顺序运行，第二个进程要在其余初始化之前就被拦下。
        // 这对飞书是硬需求 —— 长连接是集群模式且不广播，两个实例连同一个 app 会把消息
        // 随机分走，表现为「有时回有时不回」。托盘常驻放大了这个风险：用户看不见窗口，
        // 以为没开，又双击一次图标。回调顺带充当「双击图标唤起已隐藏窗口」。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        // 日志尽量早注册，这样后续插件与 setup 的输出都能落盘
        .plugin(logging::plugin(cfg.runtime_log_dir()))
        .plugin(tauri_plugin_opener::init())
        // 设置页的目录选择器。只用来挑路径 —— 读写文件仍然走沙箱约束的工具层，
        // 这个插件不给模型任何能力，它面向的是坐在电脑前的人
        .plugin(tauri_plugin_dialog::init())
        .manage(store)
        .manage(llm)
        .manage(engine)
        .manage(usage_store)
        .manage(task_store)
        .manage(channels)
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
            commands::feishu_start,
            commands::feishu_stop,
            commands::feishu_status,
            commands::feishu_test_connection,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 退出流程也要关窗口，此时必须放行，否则 prevent_close 会把退出反锁住
                if tray::is_quitting() {
                    return;
                }
                api.prevent_close();
                tray::hide_to_tray(window);
            }
        })
        .setup(move |app| {
            // 补记 Builder 之前攒下的启动日志，此刻日志插件已接管 log 宏
            for (level, message) in boot_log {
                log::log!(target: "boot", level, "{message}");
            }

            // eager 预热：启动时后台加载，不阻塞主线程；失败降级（首次使用时 get_sync 再试）
            if cfg.embedding.load_mode == config::EmbeddingLoadMode::Eager {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = handle.warmup().await {
                        log::warn!(target: "embedding", "预热失败，将在首次使用时重试: {e}");
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

            // 飞书渠道：配置开启且凭证齐全才自启，否则静默跳过
            if feishu_cfg.is_ready() {
                match channel_manager.start(feishu_cfg, channel_engine) {
                    Ok(()) => log::info!(target: "feishu", "渠道已随应用自启"),
                    Err(e) => log::error!(target: "feishu", "渠道自启失败: {e}"),
                }
            }

            // 托盘：关窗口后靠它承载进程的可见性与控制入口。
            // 放在渠道自启之后，这样首次 refresh 就能读到真实状态
            tray::setup(app.handle())?;

            log::info!(target: "app", "Avalon {} 启动完成", env!("CARGO_PKG_VERSION"));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
