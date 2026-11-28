// IPC：视觉能力状态、图片观察与缩略图。

use std::sync::Arc;

use tauri::State;

use crate::vision::{Observation, VisionCapability, VisionService};

/// 视觉能力状态：前端据此决定是否显示附件入口
#[tauri::command]
pub fn vision_status(vision: State<'_, Arc<VisionService>>) -> VisionCapability {
    vision.capability()
}

/// 对单张已落盘图片生成（或读取缓存的）观察文本
#[tauri::command]
pub async fn analyze_image(
    path: String,
    vision: State<'_, Arc<VisionService>>,
) -> Result<Observation, String> {
    vision.observe(&path).await.map_err(|e| format!("{e:#}"))
}

/// 返回缩略图相对路径（必要时生成一次），供需要路径的调用方使用
#[tauri::command]
pub async fn get_thumbnail(
    path: String,
    vision: State<'_, Arc<VisionService>>,
) -> Result<String, String> {
    vision.thumbnail(&path).await.map_err(|e| format!("{e:#}"))
}

/// 返回缩略图 data URL（必要时生成一次）。WebView 直接渲染，
/// 不依赖 Tauri asset 协议，因而也不受可配置 file_root 的 scope 限制
#[tauri::command]
pub async fn get_thumbnail_data_url(
    path: String,
    vision: State<'_, Arc<VisionService>>,
) -> Result<String, String> {
    vision
        .thumbnail_data_url(&path)
        .await
        .map_err(|e| format!("{e:#}"))
}
