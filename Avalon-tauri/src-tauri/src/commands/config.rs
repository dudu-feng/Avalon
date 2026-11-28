// IPC：配置读写、模型切换与应用初始化。

use std::sync::Arc;

use tauri::State;

use crate::config::{AppConfig, ConfigStore};
use crate::vision::VisionService;

/// 获取当前配置快照
#[tauri::command]
pub fn get_config(store: State<'_, ConfigStore>) -> Result<AppConfig, String> {
    Ok(store.get())
}

/// 切换活跃模型（校验存在后写回配置），返回校验警告列表
#[tauri::command]
pub fn set_active_model(
    name: String,
    store: State<'_, ConfigStore>,
) -> Result<Vec<String>, String> {
    store.set_active_model(&name).map_err(|e| e.to_string())
}

/// 保存配置（写回 Avalon-config.toml），返回校验警告列表
#[tauri::command]
pub fn save_config(
    mut new_config: AppConfig,
    store: State<'_, ConfigStore>,
    vision: State<'_, Arc<VisionService>>,
) -> Result<Vec<String>, String> {
    // 前端不会传 config_path（serde(skip)），继承当前值
    let current = store.get();
    new_config.config_path = current.config_path;

    // 关闭 OCR、换引擎或换模型目录后，旧会话不再复用；释放已加载的 ONNX 会话
    let ocr_changed = current.vision.ocr.enabled != new_config.vision.ocr.enabled
        || current.vision.ocr.engine != new_config.vision.ocr.engine
        || current.vision.ocr.model_dir != new_config.vision.ocr.model_dir;

    let warnings = store.save(new_config).map_err(|e| e.to_string())?;
    if ocr_changed {
        vision.unload_ocr();
        log::info!(target: "vision", "OCR 配置已变更，已卸载已加载的引擎");
    }
    Ok(warnings)
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
        log::warn!(target: "config", "配置校验: {w}");
    }
    Ok(warnings)
}
