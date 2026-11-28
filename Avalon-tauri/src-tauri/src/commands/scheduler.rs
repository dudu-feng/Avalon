// IPC：定时任务管理。

use std::sync::Arc;

use tauri::State;

use crate::scheduler::{
    parse_schedule, ScheduledTask, TaskNotifyChannel, TaskSource, TaskStore,
};

/// 创建定时任务（用户 UI 入口，source=User）。参数扁平化，返回完整任务。
#[tauri::command]
pub fn create_scheduled_task(
    name: String,
    prompt: String,
    schedule_type: String,
    schedule_value: String,
    notify_channel: Option<String>,
    notify_target: Option<String>,
    store: State<'_, Arc<TaskStore>>,
) -> Result<ScheduledTask, String> {
    let schedule = parse_schedule(&schedule_type, &schedule_value).map_err(|e| e.to_string())?;
    let notify_channel =
        TaskNotifyChannel::parse(notify_channel.as_deref().unwrap_or("none"))
            .map_err(|e| e.to_string())?;
    store
        .create(
            TaskSource::User,
            &name,
            &prompt,
            schedule,
            notify_channel,
            notify_target.as_deref(),
        )
        .map_err(|e| e.to_string())
}

/// 列出全部定时任务（创建时间倒序）
#[tauri::command]
pub fn list_scheduled_tasks(
    store: State<'_, Arc<TaskStore>>,
) -> Result<Vec<ScheduledTask>, String> {
    Ok(store.list())
}

/// 删除定时任务
#[tauri::command]
pub fn delete_scheduled_task(
    task_id: String,
    store: State<'_, Arc<TaskStore>>,
) -> Result<(), String> {
    store.delete(&task_id).map_err(|e| e.to_string())
}

/// 停用 / 启用定时任务（不删除）
#[tauri::command]
pub fn toggle_scheduled_task(
    task_id: String,
    enabled: bool,
    store: State<'_, Arc<TaskStore>>,
) -> Result<(), String> {
    store.toggle(&task_id, enabled).map_err(|e| e.to_string())
}

/// 编辑定时任务定义（名称/内容/触发方式），id 不变，执行历史保留
#[tauri::command]
#[allow(clippy::too_many_arguments)] // IPC 字段与前端表单一一对应，打包成结构体会多一层协议
pub fn update_scheduled_task(
    task_id: String,
    name: String,
    prompt: String,
    schedule_type: String,
    schedule_value: String,
    notify_channel: Option<String>,
    notify_target: Option<String>,
    store: State<'_, Arc<TaskStore>>,
) -> Result<ScheduledTask, String> {
    let schedule = parse_schedule(&schedule_type, &schedule_value).map_err(|e| e.to_string())?;
    let notify_channel =
        TaskNotifyChannel::parse(notify_channel.as_deref().unwrap_or("none"))
            .map_err(|e| e.to_string())?;
    store
        .update(
            &task_id,
            &name,
            &prompt,
            schedule,
            notify_channel,
            notify_target.as_deref(),
        )
        .map_err(|e| e.to_string())
}

/// 清除某任务的未读标记（前端查看执行历史后调用）
#[tauri::command]
pub fn mark_task_read(task_id: String, store: State<'_, Arc<TaskStore>>) -> Result<(), String> {
    store.mark_read(&task_id).map_err(|e| e.to_string())
}

/// 全部任务的未读执行总数（驱动侧边栏角标）
#[tauri::command]
pub fn get_unread_task_count(store: State<'_, Arc<TaskStore>>) -> Result<usize, String> {
    Ok(store.unread_count())
}
