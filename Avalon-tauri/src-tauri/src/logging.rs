// 运行日志
//
// 打包后是 Windows GUI 程序，没有 stdout —— 所有 println!/eprintln! 会石沉大海。
// 而托盘常驻、定时任务、飞书渠道是三条无人值守的路径，出问题时日志是唯一线索。
//
// 级别约定（改造 println! 时按这个判据选）：
//   error  用户需要知道的失败：连不上、初始化失败、配置读不了
//   warn   降级但仍在工作：表情熔断、卡片推送失败回退纯文本、拿不到 bot open_id
//   info   生命周期事件：渠道启停、定时任务触发与完成、自动压缩触发
//   debug  详细流程：帧收发、卡片每次推送、每轮 ReAct
//   trace  原始数据：帧内容、HTTP body
//
// 凭证（app_secret / api_key / tenant_access_token）一律不得进日志。

use std::path::PathBuf;

use tauri_plugin_log::{Builder, Target, TargetKind, TimezoneStrategy};

/// 单文件上限。飞书在 debug 级别下帧收发很密，5MB 大约能存几小时
const MAX_FILE_SIZE: u128 = 5 * 1024 * 1024;

/// 构建日志插件。
///
/// 只输出到文件与 stdout：webview target 会把每条日志推进前端 console，
/// 在没有日志查看页消费它之前只会干扰前端调试，等做了页面再加。
pub fn plugin(dir: PathBuf) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    // dev 下开 debug 看细节；打包后默认 info，否则日志量太大且没人看
    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    // Folder target 不保证会自己建目录。建不出来（盘不存在、权限不足）时
    // 退化成只有 stdout —— 日志没了总好过应用起不来
    let file_target = match std::fs::create_dir_all(&dir) {
        Ok(()) => Some(Target::new(TargetKind::Folder {
            path: dir,
            file_name: Some("avalon".to_string()),
        })),
        Err(e) => {
            eprintln!("[Log] 创建日志目录失败，本次只输出到控制台: {e}");
            None
        }
    };

    let mut builder = Builder::new().target(Target::new(TargetKind::Stdout));
    if let Some(target) = file_target {
        builder = builder.target(target);
    }

    builder
        .level(level)
        // 依赖库的日志（reqwest/tungstenite/hyper）在 debug 下会淹没自己的日志
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .level_for("hyper", log::LevelFilter::Warn)
        .level_for("reqwest", log::LevelFilter::Warn)
        .level_for("tungstenite", log::LevelFilter::Warn)
        .level_for("tokio_tungstenite", log::LevelFilter::Warn)
        .max_file_size(MAX_FILE_SIZE)
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .build()
}
