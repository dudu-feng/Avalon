// IPC：灵魂 / 画像条目管理。

use std::sync::Arc;

use tauri::State;

use crate::prompt::PromptAssembler;
use crate::soul::{EntryCategory, SoulEntry, SoulEntryDetail, SoulRegistry};

/// 列出可见灵魂条目（不含 system 基本设定，前端渲染条目列表）
#[tauri::command]
pub fn list_soul_entries(registry: State<'_, Arc<SoulRegistry>>) -> Result<Vec<SoulEntry>, String> {
    registry.list_visible().map_err(|e| e.to_string())
}

/// 读取单条灵魂条目 + 正文（前端模态框 / agent 查看内容）
#[tauri::command]
pub fn get_soul_entry(
    id: String,
    registry: State<'_, Arc<SoulRegistry>>,
) -> Result<SoulEntryDetail, String> {
    registry.get(&id).map_err(|e| e.to_string())
}

/// 切换条目是否注入 system prompt，写后刷新提示词缓存使下一轮对话生效
#[tauri::command]
pub fn set_soul_entry_enabled(
    id: String,
    enabled: bool,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<SoulEntry, String> {
    let entry = registry
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(entry)
}

/// 新增灵魂条目（kind=user），写后刷新提示词缓存使下一轮对话生效
#[tauri::command]
pub fn create_soul_entry(
    category: String,
    title: String,
    content: String,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<SoulEntry, String> {
    let category = EntryCategory::parse(&category)
        .ok_or_else(|| "category 应为 soul 或 profile".to_string())?;
    let entry = registry
        .create(category, &title, &content)
        .map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(entry)
}

/// 编辑灵魂条目（title/content 至少一个；system 只读），写后刷新提示词缓存
#[tauri::command]
pub fn update_soul_entry(
    id: String,
    title: Option<String>,
    content: Option<String>,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<SoulEntry, String> {
    let entry = registry
        .update(&id, title.as_deref(), content.as_deref())
        .map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(entry)
}

/// 删除灵魂条目（system 与 seed 不可删），写后刷新提示词缓存
#[tauri::command]
pub fn delete_soul_entry(
    id: String,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<(), String> {
    registry.delete(&id).map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(())
}
