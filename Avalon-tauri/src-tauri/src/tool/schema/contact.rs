// 通讯录工具 schema。

use serde_json::json;

use super::ToolDef;

/// 通讯录工具定义。
///
/// 记录/更新/查询自己对所有上下文开放；枚举通讯录和查主人通讯录只对主人开放。
/// 通讯录不挂在消息准入流程上，是否记录完全由模型决定。
pub(crate) fn contact_tool_defs(is_owner: bool) -> Vec<ToolDef> {
    let mut defs = vec![
        ToolDef {
            name: "record_user",
            description: "向通讯录记录一个新用户。只在遇到值得长期记住的新用户时调用。\
                          已存在的用户会拒绝，请改用 update_user。id 建议直接用当前消息来源里的\
                          sender_id（形如 feishu:ou_xxx / qq:xxx）；裸 id 会按当前渠道补前缀。\
                          name 是显示名，profile 是用户画像（身份、关注点、沟通偏好等）。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "用户 id，如 feishu:ou_xxx；裸 id 按当前渠道补前缀"},
                    "name": {"type": "string", "description": "显示名；不知道时可先用占位名"},
                    "profile": {"type": "string", "description": "可选，用户画像"}
                },
                "required": ["id", "name"]
            }),
        },
        ToolDef {
            name: "update_user",
            description: "更新通讯录里已有用户的 name / profile。profile 是整体替换，不是追加；\
                          要保留旧内容，先 get_user 读出再写完整画像。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "用户 id"},
                    "name": {"type": "string", "description": "可选，新显示名"},
                    "profile": {"type": "string", "description": "可选，完整用户画像（整体替换）"}
                },
                "required": ["id"]
            }),
        },
        ToolDef {
            name: "get_user",
            description: "查询通讯录里某个用户的详情（id、name、profile）。\
                          优先用 id；name 只在主人上下文可用，重名时返回候选。\
                          非主人只能查询自己的记录。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "用户 id"},
                    "name": {"type": "string", "description": "可选，按名字查（仅主人）"}
                }
            }),
        },
    ];

    if is_owner {
        defs.push(ToolDef {
            name: "list_users",
            description: "列出通讯录里已记录的用户（id、name、画像摘要），支持按关键词过滤。\
                          只用于主人查看，不会主动拉取群成员。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "可选：按 id / name / 画像关键词模糊匹配"},
                    "limit": {"type": "integer", "description": "默认 20，最大 100"}
                }
            }),
        });
        defs.push(ToolDef {
            name: "get_owner_contact",
            description: "获取主人的通讯录详情。主人由渠道在首次私聊时自动记入通讯录，\
                          只有 id 与昵称；主人画像由灵魂模块维护，这里不返回画像。"
                .to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        });
    }

    defs
}
