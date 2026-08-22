// 定时任务工具实现（给 agent 开放的双入口之一）
//
// create_scheduled_task / list_scheduled_tasks / delete_scheduled_task，
// 签名对齐 (args: &Value, store: &TaskStore) -> String（参考 memory_tools 带依赖注入）。
// 护栏由 TaskStore::create 统一兜底（prompt 长度 + agent 数量上限）。
// 参数扁平化：schedule_type（once/daily/weekly）+ schedule_value（时间点/时刻），降低 LLM 填错概率。

use serde_json::Value;

use crate::scheduler::{parse_schedule, TaskSource, TaskStore};

/// 创建定时任务（source=Agent）。返回任务 id 或错误文本。
pub fn create_scheduled_task(args: &Value, store: &TaskStore) -> String {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return "参数错误: 缺少 name 或类型应为字符串".to_string();
    };
    let Some(prompt) = args.get("prompt").and_then(Value::as_str) else {
        return "参数错误: 缺少 prompt 或类型应为字符串".to_string();
    };
    let Some(schedule_type) = args.get("schedule_type").and_then(Value::as_str) else {
        return "参数错误: 缺少 schedule_type（once / daily / weekly）".to_string();
    };
    let Some(schedule_value) = args.get("schedule_value").and_then(Value::as_str) else {
        return "参数错误: 缺少 schedule_value".to_string();
    };

    let schedule = match parse_schedule(schedule_type, schedule_value) {
        Ok(s) => s,
        Err(e) => return format!("参数错误: {e}"),
    };

    match store.create(TaskSource::Agent, name, prompt, schedule) {
        Ok(task) => format!("已创建定时任务，id = {}", task.id),
        Err(e) => format!("创建定时任务失败: {e}"),
    }
}

/// 列出全部定时任务（供 agent 先查避免重复创建）
pub fn list_scheduled_tasks(_args: &Value, store: &TaskStore) -> String {
    let tasks = store.list();
    if tasks.is_empty() {
        return "当前无定时任务".to_string();
    }
    let mut out = String::new();
    for (i, t) in tasks.iter().enumerate() {
        out.push_str(&format!(
            "[{}] id={} source={:?} schedule={:?} enabled={} prompt={}",
            i + 1, t.id, t.source, t.schedule, t.enabled, t.prompt
        ));
        if i + 1 < tasks.len() {
            out.push('\n');
        }
    }
    out
}

/// 删除定时任务（agent 撤销自己创建的任务；不区分来源）
pub fn delete_scheduled_task(args: &Value, store: &TaskStore) -> String {
    let Some(task_id) = args.get("task_id").and_then(Value::as_str) else {
        return "参数错误: 缺少 task_id".to_string();
    };
    match store.delete(task_id) {
        Ok(()) => format!("已删除定时任务 {task_id}"),
        Err(e) => format!("删除定时任务失败: {e}"),
    }
}
