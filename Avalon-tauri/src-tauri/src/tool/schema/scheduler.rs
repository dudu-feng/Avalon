// 定时任务工具 schema。

use serde_json::json;

use super::ToolDef;

/// 定时任务工具定义（仅注入 TaskStore 时暴露，3 个）
pub(crate) fn scheduler_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "create_scheduled_task",
            description: "创建一个定时任务（在指定时间自动执行一次对话）。schedule_type 为 once/daily/weekly；once 的 schedule_value 形如 'YYYY-MM-DD HH:MM'，daily 为 'HH:MM'，weekly 为 'N HH:MM'（N=1 周一 .. 7 周日）".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "任务名称（简短标题）"},
                    "prompt": {"type": "string", "description": "任务内容（每次触发时喂给 agent 的输入）"},
                    "schedule_type": {"type": "string", "enum": ["once", "daily", "weekly"], "description": "触发方式"},
                    "schedule_value": {"type": "string", "description": "触发时间：once=YYYY-MM-DD HH:MM；daily=HH:MM；weekly=N HH:MM"},
                    "notify_channel": {"type": "string", "enum": ["none", "desktop", "feishu", "qq"], "description": "结果通知渠道：none 只写入会话；desktop 桌面通知；feishu / qq 发给主人。任务内容已自行通知时选 none"},
                    "notify_target": {"type": "string", "description": "可选通知目标（飞书 chat_id/open_id、QQ openid 或 group:群id）；缺省发给主人"}
                },
                "required": ["name", "prompt", "schedule_type", "schedule_value"]
            }),
        },
        ToolDef {
            name: "list_scheduled_tasks",
            description: "列出全部定时任务（id、来源、触发时间、内容、是否启用），供创建前查重".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: "delete_scheduled_task",
            description: "删除指定 id 的定时任务".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "任务 id"}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "update_scheduled_task",
            description: "编辑指定 id 的定时任务（名称、内容或触发时间）。task_id 用 list_scheduled_tasks 查得；schedule_type/schedule_value 格式同 create_scheduled_task".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "任务 id"},
                    "name": {"type": "string", "description": "任务名称（简短标题）"},
                    "prompt": {"type": "string", "description": "任务内容（每次触发时喂给 agent 的输入）"},
                    "schedule_type": {"type": "string", "enum": ["once", "daily", "weekly"], "description": "触发方式"},
                    "schedule_value": {"type": "string", "description": "触发时间：once=YYYY-MM-DD HH:MM；daily=HH:MM；weekly=N HH:MM"},
                    "notify_channel": {"type": "string", "enum": ["none", "desktop", "feishu", "qq"], "description": "结果通知渠道：none 只写入会话；desktop 桌面通知；feishu / qq 发给主人"},
                    "notify_target": {"type": "string", "description": "可选通知目标；缺省发给主人"}
                },
                "required": ["task_id", "name", "prompt", "schedule_type", "schedule_value"]
            }),
        },
    ]
}
