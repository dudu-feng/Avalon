// QQ 官方机器人事件解析
//
// 平台的事件字段在不同账号/版本间有轻微差异（例如 @ 消息可能走
// GROUP_AT_MESSAGE_CREATE，也可能走 GROUP_MESSAGE_CREATE），解析层一律容错：
// 缺字段、类型不符都当作「没有」而不是 panic；未知字段直接忽略。
//
// 用户身份是 openid，不是 QQ 号；openid 按应用隔离，换 AppID 后白名单必须重配。

use serde_json::Value;

/// 群 @ 消息事件
pub const GROUP_AT_MESSAGE_CREATE: &str = "GROUP_AT_MESSAGE_CREATE";
/// 群全量消息事件。2026 年起部分账号的 @ 消息也走这个事件
pub const GROUP_MESSAGE_CREATE: &str = "GROUP_MESSAGE_CREATE";
/// 单聊消息事件
pub const C2C_MESSAGE_CREATE: &str = "C2C_MESSAGE_CREATE";

/// QQ 会话类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatType {
    Group,
    C2c,
}

impl ChatType {
    pub fn as_str(self) -> &'static str {
        match self {
            ChatType::Group => "group",
            ChatType::C2c => "c2c",
        }
    }

    pub fn is_group(self) -> bool {
        self == ChatType::Group
    }
}

/// 发送目标：群 openid 或单聊 user openid
#[derive(Debug, Clone)]
pub struct Target {
    pub chat_type: ChatType,
    pub chat_id: String,
}

impl Target {
    pub fn new(chat_type: ChatType, chat_id: impl Into<String>) -> Self {
        Self {
            chat_type,
            chat_id: chat_id.into(),
        }
    }
}

/// 网关下发的业务事件，d 是事件体原文
#[derive(Debug, Clone)]
pub struct GatewayEvent {
    /// 事件 id，用于去重
    pub id: Option<String>,
    /// 事件类型，如 GROUP_AT_MESSAGE_CREATE
    pub t: String,
    pub d: Value,
}

/// 图片附件。只保留下载所需的最小信息，签名 URL 不写日志
#[derive(Debug, Clone)]
pub struct ImageAttachment {
    pub url: String,
    pub filename: String,
}

/// 一条已解析的 QQ 消息
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub message_id: String,
    pub chat_type: ChatType,
    pub chat_id: String,
    pub sender_open_id: String,
    /// 发送者昵称。平台给的时候才有，群事件通常是 author.username
    pub sender_name: Option<String>,
    pub text: String,
    pub images: Vec<ImageAttachment>,
    /// 是否 @ 了当前机器人。群 @ 事件恒为 true；全量事件看 mentions[].is_you
    pub mentioned_bot: bool,
    /// 收到的附件总数（含非图片），用于判断「不支持的消息类型」
    pub attachment_count: usize,
    /// 引用气泡用的消息 id（REFIDX_ 开头），来自 message_scene.ext。
    /// 它和 message_id 不是一回事：主动发送时 message_reference 只认这个
    pub msg_idx: Option<String>,
}

impl IncomingMessage {
    pub fn is_group(&self) -> bool {
        self.chat_type.is_group()
    }

    /// 待合并图片的分组 key。始终按聊天局部化，与 session_mode 无关
    pub fn pending_key(&self) -> String {
        crate::channel::common::sanitize_channel(&format!(
            "qq_chat_{}_{}",
            self.chat_type.as_str(),
            self.chat_id
        ))
    }

    /// 落盘用的结构化来源信息。
    /// sender_id 是带渠道前缀的规范 id（qq:xxx），供通讯录工具判断当前是谁；
    /// sender 保留裸 openid 兼容旧会话。
    pub fn meta(&self) -> Value {
        let mut meta = serde_json::json!({
            "message_id": self.message_id,
            "chat_id": self.chat_id,
            "chat_type": self.chat_type.as_str(),
            "sender": self.sender_open_id,
            "sender_id": format!("qq:{}", self.sender_open_id),
            "content_type": if self.images.is_empty() { "text" } else { "image" },
        });
        // 引用 id 不是每条消息都有，缺省时不写空字段
        if let Some(msg_idx) = &self.msg_idx {
            meta["msg_idx"] = Value::String(msg_idx.clone());
        }
        if let Some(name) = &self.sender_name {
            meta["sender_name"] = Value::String(name.clone());
        }
        meta
    }

    /// 仅本轮注入给模型的来源提示，不落盘。
    /// 带上 msg_idx，模型想引用这条消息时才知道该传什么
    pub fn source_hint_with(&self, text: &str) -> String {
        let scene = if self.is_group() {
            "QQ群聊"
        } else {
            "QQ私聊"
        };
        match &self.msg_idx {
            Some(idx) => format!(
                "[{scene} | qq:{} | msg_idx:{idx}]\n{text}",
                self.sender_open_id
            ),
            None => format!("[{scene} | qq:{}]\n{text}", self.sender_open_id),
        }
    }
}

/// 判断事件是否是消息事件
pub fn is_message_event(event_type: &str) -> bool {
    matches!(
        event_type,
        GROUP_AT_MESSAGE_CREATE | GROUP_MESSAGE_CREATE | C2C_MESSAGE_CREATE
    )
}

/// 从事件体解析消息。不是消息事件、缺少关键字段时返回 None
pub fn parse_message(event_type: &str, d: &Value) -> Option<IncomingMessage> {
    let (chat_type, chat_id, sender_open_id, sender_name, mentioned_bot) = match event_type {
        GROUP_AT_MESSAGE_CREATE | GROUP_MESSAGE_CREATE => {
            let chat_id = str_at(d, &["group_openid"]).or_else(|| str_at(d, &["group_id"]))?;
            let sender = str_at(d, &["author", "member_openid"])
                .or_else(|| str_at(d, &["author", "id"]))
                .unwrap_or_default();
            // @ 事件通常没有 mentions，按已 @ 处理；有 mentions 时以 is_you 为准，
            // 否则同群多机器人会一起抢答
            let mentioned = mentioned_bot(d, event_type);
            (ChatType::Group, chat_id, sender, sender_name(d), mentioned)
        }
        C2C_MESSAGE_CREATE => {
            let chat_id =
                str_at(d, &["author", "user_openid"]).or_else(|| str_at(d, &["author", "id"]))?;
            let sender = str_at(d, &["author", "user_openid"])
                .or_else(|| str_at(d, &["author", "id"]))
                .unwrap_or_default();
            (ChatType::C2c, chat_id, sender, sender_name(d), true)
        }
        _ => return None,
    };

    let contents = d.get("content").and_then(Value::as_str).unwrap_or_default();
    let text = strip_mentions(contents).trim().to_string();

    let attachments = d
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let attachment_count = attachments.len();

    let images = attachments
        .iter()
        .filter_map(parse_image)
        .collect::<Vec<_>>();

    let message_id = str_at(d, &["id"]).unwrap_or_default();

    Some(IncomingMessage {
        message_id,
        chat_type,
        chat_id,
        sender_open_id,
        sender_name,
        text,
        images,
        mentioned_bot,
        attachment_count,
        msg_idx: parse_msg_idx(d),
    })
}

/// 从 message_scene.ext 提取引用 id。
///
/// 平台给的是字符串数组，常见形如 `["msg_idx=REFIDX_xxx"]`；也兼容直接给
/// `REFIDX_xxx` 的版本。取不到就是没有 —— 引用降级为普通被动回复，不猜测。
fn parse_msg_idx(d: &Value) -> Option<String> {
    let ext = d.get("message_scene")?.get("ext")?.as_array()?;
    ext.iter().filter_map(Value::as_str).find_map(|raw| {
        let raw = raw.trim();
        let value = raw.strip_prefix("msg_idx=").unwrap_or(raw).trim();
        value.starts_with("REFIDX_").then(|| value.to_string())
    })
}

/// 从附件列表里取图片。只认 content_type 以 image/ 开头且有 URL 的项
fn parse_image(item: &Value) -> Option<ImageAttachment> {
    let content_type = item
        .get("content_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !content_type.starts_with("image/") {
        return None;
    }
    let url = item.get("url").and_then(Value::as_str)?.trim();
    if url.is_empty() {
        return None;
    }
    let filename = item
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(ImageAttachment {
        url: url.to_string(),
        filename,
    })
}

/// 是否 @ 了当前机器人。
///
/// 优先级：is_you > bot > 事件类型回退。`is_you` 是新协议区分「@ 的是哪一个
/// 机器人」的唯一可靠字段；只看 `bot = true` 会让同群所有机器人同时响应。
fn mentioned_bot(d: &Value, event_type: &str) -> bool {
    let Some(mentions) = d.get("mentions").and_then(Value::as_array) else {
        return event_type == GROUP_AT_MESSAGE_CREATE;
    };
    if mentions.is_empty() {
        return event_type == GROUP_AT_MESSAGE_CREATE;
    }

    let mut saw_is_you = false;
    let mut saw_bot = false;
    for m in mentions {
        match m.get("is_you").and_then(Value::as_bool) {
            Some(true) => return true,
            Some(false) => saw_is_you = true,
            None => {}
        }
        if m.get("bot").and_then(Value::as_bool) == Some(true) {
            saw_bot = true;
        }
    }

    // 有明确的 is_you = false，说明 @ 的是别的机器人
    if saw_is_you {
        return false;
    }
    // 旧协议没有 is_you：GROUP_AT_MESSAGE_CREATE 本身就是「@ 了本机器人」的信号，
    // 只有全量群消息事件才退回 bot 标记
    if event_type == GROUP_AT_MESSAGE_CREATE {
        return true;
    }
    saw_bot
}

/// 去掉正文里的 <@xxx> / <@all> 占位符
fn strip_mentions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '<' {
            let rest: String = chars
                .clone()
                .take_while(|(_, c)| *c != '>')
                .map(|(_, c)| c)
                .collect();
            if rest.starts_with('@') {
                // 吃掉直到 '>'（含）
                for (_, c) in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// 发送者昵称。不同事件版本字段名不一致，按常见顺序取第一个非空值
fn sender_name(d: &Value) -> Option<String> {
    str_at(d, &["author", "username"]).or_else(|| str_at(d, &["author", "nickname"]))
}

fn str_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut cur = value;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_at_event() -> Value {
        serde_json::json!({
            "id": "MSG1",
            "content": " <@!ABC> 看看这个",
            "group_openid": "GROUP1",
            "author": { "member_openid": "USER1" },
            "attachments": [
                { "content_type": "image/png", "url": "https://multimedia.nt.qq.com.cn/a.png", "filename": "a.png" },
                { "content_type": "file", "url": "https://example.com/a.txt" }
            ]
        })
    }

    #[test]
    fn parses_group_at_message() {
        let msg = parse_message(GROUP_AT_MESSAGE_CREATE, &group_at_event()).expect("应解析成功");
        assert_eq!(msg.chat_type, ChatType::Group);
        assert_eq!(msg.chat_id, "GROUP1");
        assert_eq!(msg.sender_open_id, "USER1");
        assert_eq!(msg.text, "看看这个");
        assert!(msg.mentioned_bot, "@ 事件应视为已 @");
        assert_eq!(msg.images.len(), 1);
        assert_eq!(msg.attachment_count, 2);
    }

    #[test]
    fn parses_sender_name_when_platform_provides_it() {
        let mut d = group_at_event();
        d["author"]["username"] = serde_json::json!("小明");
        let msg = parse_message(GROUP_AT_MESSAGE_CREATE, &d).expect("应解析成功");
        assert_eq!(msg.sender_name.as_deref(), Some("小明"));
        assert_eq!(msg.meta()["sender_name"], "小明");

        let msg = parse_message(GROUP_AT_MESSAGE_CREATE, &group_at_event()).expect("应解析成功");
        assert!(msg.sender_name.is_none());
        assert!(msg.meta().get("sender_name").is_none());
    }

    #[test]
    fn parses_c2c_message() {
        let d = serde_json::json!({
            "id": "MSG2",
            "content": "你好",
            "author": { "user_openid": "USER2" }
        });
        let msg = parse_message(C2C_MESSAGE_CREATE, &d).expect("应解析成功");
        assert_eq!(msg.chat_type, ChatType::C2c);
        assert_eq!(msg.chat_id, "USER2");
        assert_eq!(msg.text, "你好");
        assert!(msg.mentioned_bot);
    }

    #[test]
    fn group_full_message_requires_is_you() {
        let mut d = group_at_event();
        d["mentions"] = serde_json::json!([
            { "bot": true, "is_you": false }
        ]);
        let msg = parse_message(GROUP_MESSAGE_CREATE, &d).expect("应解析成功");
        assert!(!msg.mentioned_bot, "明确 @ 的是别的机器人，不应响应");

        d["mentions"] = serde_json::json!([
            { "bot": true, "is_you": true }
        ]);
        let msg = parse_message(GROUP_MESSAGE_CREATE, &d).expect("应解析成功");
        assert!(msg.mentioned_bot);
    }

    #[test]
    fn group_at_event_trusts_event_type_when_is_you_missing() {
        let mut d = group_at_event();
        // 旧协议没有 is_you，只给了 bot 标记甚至什么都没有
        d["mentions"] = serde_json::json!([{ "id": "SOMEONE" }]);
        let msg = parse_message(GROUP_AT_MESSAGE_CREATE, &d).expect("应解析成功");
        assert!(msg.mentioned_bot, "@ 事件没有 is_you 时应按已 @ 处理");
    }

    #[test]
    fn full_message_without_mentions_is_not_mentioned() {
        let mut d = group_at_event();
        d["mentions"] = serde_json::json!([]);
        let msg = parse_message(GROUP_MESSAGE_CREATE, &d).expect("应解析成功");
        assert!(!msg.mentioned_bot);
    }

    #[test]
    fn parses_msg_idx_from_message_scene() {
        let mut d = group_at_event();
        d["message_scene"] = serde_json::json!({ "ext": ["msg_idx=REFIDX_abc"] });
        let msg = parse_message(GROUP_AT_MESSAGE_CREATE, &d).expect("应解析成功");
        assert_eq!(msg.msg_idx.as_deref(), Some("REFIDX_abc"));
        assert_eq!(msg.meta()["msg_idx"], "REFIDX_abc");
        assert!(msg.source_hint_with("hi").contains("msg_idx:REFIDX_abc"));
    }

    #[test]
    fn missing_msg_idx_leaves_meta_clean() {
        let msg = parse_message(GROUP_AT_MESSAGE_CREATE, &group_at_event()).expect("应解析成功");
        assert!(msg.msg_idx.is_none());
        assert!(msg.meta().get("msg_idx").is_none());
    }

    #[test]
    fn strips_multiple_mentions() {
        assert_eq!(strip_mentions("<@a> hi <@all>"), " hi ");
        assert_eq!(strip_mentions("a < b"), "a < b");
    }

    #[test]
    fn unknown_event_is_ignored() {
        assert!(parse_message("GROUP_ADD_ROBOT", &group_at_event()).is_none());
    }
}
