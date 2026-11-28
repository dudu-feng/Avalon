// IPC：聊天附件落盘、路径解析与过期文件清理。

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::State;

use crate::config::ConfigStore;
use crate::file::{
    CleanupReport, FileSource, FileStore, SaveRequest, StoredFile, MAX_FILE_BYTES,
    QUARANTINE_TTL_DAYS, TEMP_TTL_HOURS,
};

/// 保存聊天附件。
///
/// WebView 里的 File 没有可读的本地路径，所以只能传 base64。解码、哈希与落盘
/// 都是阻塞操作，放进 spawn_blocking，避免 50MB 附件卡住主线程。
#[tauri::command]
pub async fn save_attachment(
    file_name: String,
    mime: String,
    data_base64: String,
    source: String,
    session_id: Option<String>,
    message_id: Option<String>,
    store: State<'_, Arc<FileStore>>,
) -> Result<StoredFile, String> {
    let source = FileSource::parse(&source).ok_or_else(|| format!("未知的文件来源: {source}"))?;

    // 先按 base64 长度预检：超限直接拒绝，不必解码一份 50MB 的字节
    if data_base64.len() as u64 > MAX_FILE_BYTES.saturating_mul(4) / 3 + 16 {
        return Err(format!("文件超过上限 {} MB", MAX_FILE_BYTES / 1024 / 1024));
    }

    let bytes = STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("文件超过上限 {} MB", MAX_FILE_BYTES / 1024 / 1024));
    }

    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || {
        store
            .save(SaveRequest {
                bytes: &bytes,
                original_name: &file_name,
                declared_mime: Some(&mime),
                source,
                session_id: session_id.as_deref(),
                message_id: message_id.as_deref(),
            })
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("保存任务失败: {e}"))?
}

/// 相对路径 → 绝对路径（已做越界校验），供前端预览或交给系统程序打开
#[tauri::command]
pub fn resolve_file_path(
    relative: String,
    store: State<'_, Arc<FileStore>>,
) -> Result<String, String> {
    store
        .resolve(&relative)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|e| format!("{e:#}"))
}

/// 清理过期 temp / quarantine / 视觉缓存。TTL 不传时用默认值或配置值
#[tauri::command]
pub async fn cleanup_files(
    temp_ttl_hours: Option<i64>,
    quarantine_ttl_days: Option<i64>,
    cache_ttl_days: Option<i64>,
    store: State<'_, Arc<FileStore>>,
    config: State<'_, ConfigStore>,
) -> Result<CleanupReport, String> {
    let hours = temp_ttl_hours.unwrap_or(TEMP_TTL_HOURS);
    let days = quarantine_ttl_days.unwrap_or(QUARANTINE_TTL_DAYS);
    // 缓存 TTL 以配置为默认值；命令参数只用于临时覆盖
    let cache_days = cache_ttl_days.unwrap_or(config.get().vision.cache_ttl_days);
    let store = store.inner().clone();

    tokio::task::spawn_blocking(move || {
        let now = chrono::Local::now();
        let mut report = store
            .cleanup_with_ttl(now, hours, days)
            .map_err(|e| format!("{e:#}"))?;
        let cache = store
            .cleanup_cache(now, cache_days)
            .map_err(|e| format!("{e:#}"))?;

        report.removed_files += cache.removed_files;
        report.removed_dirs += cache.removed_dirs;
        report.freed_bytes += cache.freed_bytes;
        Ok(report)
    })
    .await
    .map_err(|e| format!("清理任务失败: {e}"))?
}
