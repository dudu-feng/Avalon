// 灵魂条目工具 schema。

use serde_json::json;

use super::ToolDef;

/// 灵魂注册表工具定义（仅注入 SoulRegistry 时暴露，6 个）
pub(crate) fn soul_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_soul_entries",
            description:
                "列出灵魂注册表中的可见条目（id、类别、保护级别、标题，不含系统基本设定），\
                          用于查看当前灵魂结构与已有画像，增删改前先查 id"
                    .to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: "create_soul_entry",
            description: "向灵魂注册表新增一条条目（category 为 soul=灵魂 / profile=用户画像）。\
                          当出现值得长期记住的新信息、而现有条目无法容纳时调用。\
                          title 为条目标题，content 为条目内容"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "category": {"type": "string", "enum": ["soul", "profile"], "description": "soul=灵魂（自我设定），profile=用户画像（关于主人的长期记忆）"},
                    "title": {"type": "string", "description": "条目标题，如「偏好与习惯」"},
                    "content": {"type": "string", "description": "条目内容"}
                },
                "required": ["category", "title", "content"]
            }),
        },
        ToolDef {
            name: "update_soul_entry",
            description: "编辑灵魂注册表中的一条条目（title 与 content 至少传一个）。\
                          用户画像的稳定信息积累用本工具追加到已有条目；\
                          系统注入条目（kind=system）只读，不可修改"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "条目 id，用 list_soul_entries 查得"},
                    "title": {"type": "string", "description": "新标题（可选，不传则不修改）"},
                    "content": {"type": "string", "description": "新内容（可选，不传则不修改）"}
                },
                "required": ["id"]
            }),
        },
        ToolDef {
            name: "delete_soul_entry",
            description: "删除灵魂注册表中的一条 user 条目。系统注入与种子条目（kind=system/seed）\
                          不可删除。仅删除确已过时、错误的条目"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "条目 id，用 list_soul_entries 查得"}
                },
                "required": ["id"]
            }),
        },
        ToolDef {
            name: "get_soul_entry",
            description: "读取灵魂注册表中某一条目的完整内容（标题 + 正文）。\
                          list_soul_entries 只返回元数据，需要看某条的具体内容时用本工具"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "条目 id，用 list_soul_entries 查得"}
                },
                "required": ["id"]
            }),
        },
        ToolDef {
            name: "set_soul_entry_enabled",
            description: "切换灵魂注册表中某条目是否注入 system prompt（enabled）。\
                          停用后该条目不再拼进系统提示词，可再次启用恢复；\
                          系统注入条目（基本设定）不可切换"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "条目 id，用 list_soul_entries 查得"},
                    "enabled": {"type": "boolean", "description": "true=注入，false=停用"}
                },
                "required": ["id", "enabled"]
            }),
        },
    ]
}
