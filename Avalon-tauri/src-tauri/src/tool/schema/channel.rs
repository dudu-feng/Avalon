// 飞书 / QQ 发送工具 schema。

use serde_json::json;

use super::ToolDef;

/// 飞书发送工具定义（仅注入 FeishuHandle 时暴露，2 个）
pub(crate) fn feishu_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "feishu_notify_owner",
            // 「主动」两个字是重点：这是定时任务唯一能触达用户的出口
            description: "通过飞书主动给主人发一条消息。用于定时任务的结果推送、\
                          需要提醒用户的重要发现。收件人由配置决定，无需也无法指定。\
                          飞书渠道必须处于运行状态，否则会失败。\
                          注意：正在进行的飞书对话，回复由系统自动发送，不要用本工具重发。\
                          发送失败时先调用 get_owner_contact 核对主人 id，或检查设置页的渠道配置"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "消息正文，支持 markdown"}
                },
                "required": ["text"]
            }),
        },
        ToolDef {
            name: "feishu_send_to",
            description: "向指定的飞书会话发送一条消息。chat_id 从上下文里的会话信息中获取，\
                          只能是会话 id（oc_ 开头），不能是用户 id。\
                          可用 quote_message_id 引用（回复）该会话里的某条消息。\
                          注意：正在进行的飞书对话，回复由系统自动发送，\
                          本工具只用于发往其它会话或额外追加一条"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {"type": "string", "description": "目标会话 id，oc_ 开头"},
                    "text": {"type": "string", "description": "消息正文，支持 markdown"},
                    "quote_message_id": {
                        "type": "string",
                        "description": "可选：要引用的消息 id（om_ 开头，见上下文会话信息）"
                    }
                },
                "required": ["chat_id", "text"]
            }),
        },
    ]
}

/// QQ 发送工具定义（仅注入 QqHandle 时暴露，2 个）
pub(crate) fn qq_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "qq_notify_owner",
            description: "通过 QQ 单聊主动给主人发一条消息。用于定时任务结果推送、\
                          后台任务完成提醒等当前没有对话出口的场景。收件人由配置决定，无需也无法指定。\
                          QQ 渠道必须处于运行状态，否则会失败。\
                          QQ 主动消息受平台额度限制（通常要求主人近期与机器人互动过），\
                          额度不足会发送失败；不要把同一条内容拆成多条反复发送。\
                          正在进行的 QQ 对话，回复由系统自动发送，不要用本工具重发。\
                          发送失败时先调用 get_owner_contact 核对主人 id，或检查设置页的渠道配置"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "消息正文，支持 markdown"}
                },
                "required": ["text"]
            }),
        },
        ToolDef {
            name: "qq_send_to",
            description: "向指定的 QQ 群聊或单聊发送一条消息。chat_type=group 时 chat_id 是群 openid，\
                          chat_type=c2c 时是用户 openid（openid 不是 QQ 号，从上下文会话信息获取）。\
                          群聊与单聊的 openid 都是不透明字符串，必须显式指定类型。\
                          可用 quote_message_id 引用（回复）该会话里的某条消息。\
                          QQ 主动消息受平台额度限制（通常需要该会话近期与机器人互动过），\
                          额度不足会发送失败；不要把同一条内容拆成多条反复发送。\
                          正在进行的 QQ 对话，回复由系统自动发送，不要用本工具重发"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_type": {
                        "type": "string",
                        "enum": ["group", "c2c"],
                        "description": "会话类型：group=群聊，c2c=私聊"
                    },
                    "chat_id": {"type": "string", "description": "群 openid（group）或用户 openid（c2c）"},
                    "text": {"type": "string", "description": "消息正文，支持 markdown"},
                    "quote_message_id": {
                        "type": "string",
                        "description": "可选：要引用的消息 id（msg_idx，REFIDX_ 开头，见上下文会话信息）"
                    }
                },
                "required": ["chat_type", "chat_id", "text"]
            }),
        },
    ]
}
