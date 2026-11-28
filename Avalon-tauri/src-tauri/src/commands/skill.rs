// IPC：技能管理。

use std::sync::Arc;

use tauri::State;

use crate::skill::{Skill, SkillDetail, SkillRegistry};

/// 列出全部技能（name + description，正文经 get_skill 按需读取）
#[tauri::command]
pub fn list_skills(registry: State<'_, Arc<SkillRegistry>>) -> Result<Vec<Skill>, String> {
    registry.list().map_err(|e| e.to_string())
}

/// 读取单个技能正文（前端详情/编辑模态框，agent 的 use_skill 也走此读取）
#[tauri::command]
pub fn get_skill(
    name: String,
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<SkillDetail, String> {
    registry.get(&name).map_err(|e| e.to_string())
}

/// 新增技能（name 需合法：ascii 字母数字 + -/_，用作目录名）
#[tauri::command]
pub fn create_skill(
    name: String,
    description: String,
    content: String,
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<Skill, String> {
    registry
        .create(&name, &description, &content)
        .map_err(|e| e.to_string())
}

/// 编辑技能（name 只读，仅改 description + content）
#[tauri::command]
pub fn update_skill(
    name: String,
    description: String,
    content: String,
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<Skill, String> {
    registry
        .update(&name, &description, &content)
        .map_err(|e| e.to_string())
}

/// 删除技能（删除其整个目录）
#[tauri::command]
pub fn delete_skill(name: String, registry: State<'_, Arc<SkillRegistry>>) -> Result<(), String> {
    registry.delete(&name).map_err(|e| e.to_string())
}
