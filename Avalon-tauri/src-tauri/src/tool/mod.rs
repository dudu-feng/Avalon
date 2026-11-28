// Tool 工具层模块
//
// 职责：智能体基础操作工具（文件增删改查、目录列举、终端命令）+ 记忆检索工具
// （search_session_memory，接入 vector::MemoryIndex 检索契约），统一分发。
// 定义 engine 层所需的 ToolRegistry 契约（依赖反转：engine 依赖本模块的 trait，
// tool 不依赖 engine 的实现；记忆检索依赖 vector 的 MemoryIndex trait 而非具体实现）。

#![allow(dead_code)] // tool 模块部分工具供未来 engine 引用，接入后逐步移除

pub mod contact_tools;
pub mod feishu_tools;
pub mod fs_tools;
pub mod history_tools;
pub mod memory_tools;
pub mod qq_tools;
pub mod registry;
mod schema;
pub mod sandbox;
pub mod scheduler_tools;
pub mod skill_tools;
pub mod soul_tools;
pub mod vision_tools;
pub mod web_tools;

use async_trait::async_trait;
use serde_json::Value;

use crate::session::{MessageOrigin, MessageSource, TrustLevel};

/// 一次工具调用所在的上下文。
///
/// 只回答「当前是谁、在哪个渠道说话」，不参与消息路由、不落盘、不是消息信封。
/// 通讯录工具靠它判断非主人只能操作自己；渠道发送类工具靠它判断是否暴露。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolContext {
    /// 当前渠道：feishu / qq / desktop / scheduler
    pub channel: String,
    /// engine 的会话键（current/{key}.json），用于默认读取「当前会话」原文
    pub session_key: String,
    /// 当前发言人 id，带渠道前缀（如 feishu:ou_xxx）；桌面/后台任务为 None
    pub sender_id: Option<String>,
    /// 当前发言人是否通讯录记录的主人。桌面端与后台任务默认是
    pub is_owner: bool,
}

impl ToolContext {
    /// 桌面端/后台任务：没有外部渠道发言人，按主人处理
    pub fn owner_desktop() -> Self {
        Self {
            channel: "desktop".to_string(),
            session_key: "main".to_string(),
            sender_id: None,
            is_owner: true,
        }
    }

    /// 从已判定的消息来源构造工具上下文。
    ///
    /// 信任级别在 engine 层结合通讯录判定后写入 origin；工具层只消费结论，
    /// 不再重复查询通讯录，避免两处判断口径不一致。
    pub fn from_origin(origin: &MessageOrigin) -> Self {
        let channel_kind = match origin.source {
            MessageSource::Feishu => "feishu",
            MessageSource::Qq => "qq",
            MessageSource::Task => "scheduler",
            MessageSource::Desktop => "desktop",
        };
        let channel_kind = origin
            .source
            .channel_prefix()
            .unwrap_or(channel_kind);

        Self {
            channel: channel_kind.to_string(),
            session_key: "main".to_string(),
            sender_id: origin.sender_id.clone(),
            is_owner: origin.trust == TrustLevel::Owner,
        }
    }

    /// 当前渠道前缀；桌面/后台任务没有
    pub fn channel_prefix(&self) -> Option<&str> {
        match self.channel.as_str() {
            "feishu" | "qq" => Some(self.channel.as_str()),
            _ => None,
        }
    }
}

/// 工具权限级别：非主人白名单只能放行 NonOwnerSafe。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolScope {
    NonOwnerSafe,
    OwnerOnly,
}

/// 工具名 → 权限级别。新增工具时必须在这里归类，默认按 OwnerOnly 处理。
pub fn tool_scope(name: &str) -> ToolScope {
    match name {
        "web_search"
        | "read_web_page"
        | "search_session_memory"
        | "list_session_history"
        | "read_session_history" => ToolScope::NonOwnerSafe,
        // read_tool_result 读的是全局 main 会话的 artifact，无法区分归属：
        // 非主人放行会读到主人轮次的完整工具输出，因此归为 OwnerOnly。
        _ => ToolScope::OwnerOnly,
    }
}

/// 工具注册表契约：engine 通过此 trait 调用工具（不依赖具体实现）
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// 生成 OpenAI 原生 `tools` 参数（function.name + description + parameters JSON Schema）。
    /// 需要按上下文隐藏的工具（如非主人不暴露通讯录枚举）在这里过滤
    fn get_tools_schema(&self, ctx: &ToolContext) -> Vec<Value>;
    /// 按名字调用工具，统一返回字符串结果（含错误信息）
    async fn invoke_tool(&self, ctx: &ToolContext, name: &str, arguments: &Value) -> String;
}

#[allow(unused_imports)] // ToolSet 供未来 engine 显式引用
pub use registry::ToolSet;
/// 供 config::loader 校验白名单条目用（沙箱本身只在 tool 模块内部流转）
pub use sandbox::resolve_allowed_command;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_drives_channel_and_owner() {
        // 统一到 main 会话后 channel 不再带渠道名，工具上下文靠 origin 识别来源与权限
        let origin = MessageOrigin::from_value(serde_json::json!({
            "source": "feishu",
            "sender_id": "feishu:ou_owner",
        }))
        .with_trust(TrustLevel::Owner);
        let ctx = ToolContext::from_origin(&origin);
        assert_eq!(ctx.channel, "feishu");
        assert_eq!(ctx.session_key, "main");
        assert_eq!(ctx.sender_id.as_deref(), Some("feishu:ou_owner"));
        assert!(ctx.is_owner);
    }

    #[test]
    fn non_owner_origin_is_not_owner() {
        let origin = MessageOrigin::from_value(serde_json::json!({
            "source": "qq",
            "sender_id": "qq:someone_else",
        }))
        .with_trust(TrustLevel::External);
        let ctx = ToolContext::from_origin(&origin);
        assert_eq!(ctx.channel, "qq");
        assert!(!ctx.is_owner);
    }

    #[test]
    fn scheduler_and_desktop_have_no_sender() {
        let task = MessageOrigin::task_owner(Some("daily".into()), Some("每日提醒".into()));
        let ctx = ToolContext::from_origin(&task);
        assert_eq!(ctx.channel, "scheduler");
        assert!(ctx.is_owner);

        let desktop = MessageOrigin::desktop_owner();
        let ctx = ToolContext::from_origin(&desktop);
        assert_eq!(ctx.channel, "desktop");
        assert!(ctx.is_owner);
    }

    #[test]
    fn whitelist_only_accepts_non_owner_safe_tools() {
        assert_eq!(tool_scope("web_search"), ToolScope::NonOwnerSafe);
        assert_eq!(tool_scope("read_tool_result"), ToolScope::OwnerOnly);
        assert_eq!(tool_scope("write_file"), ToolScope::OwnerOnly);
        assert_eq!(tool_scope("feishu_send_message"), ToolScope::OwnerOnly);
        assert_eq!(tool_scope("unknown_tool"), ToolScope::OwnerOnly);
    }
}
