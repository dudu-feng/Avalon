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
pub mod sandbox;
pub mod scheduler_tools;
pub mod skill_tools;
pub mod soul_tools;
pub mod vision_tools;
pub mod web_tools;

use async_trait::async_trait;
use serde_json::Value;

use crate::config::ConfigStore;

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
    /// 当前发言人是否配置里的主人。桌面端与后台任务默认是
    pub is_owner: bool,
}

impl ToolContext {
    /// 桌面端/后台任务：没有外部渠道发言人，按主人处理
    pub fn owner_desktop() -> Self {
        Self {
            channel: "desktop".to_string(),
            session_key: "app".to_string(),
            sender_id: None,
            is_owner: true,
        }
    }

    /// 从会话 key + 消息 meta + 配置推导工具上下文
    pub fn from_input(channel: &str, meta: Option<&Value>, config: &ConfigStore) -> Self {
        let cfg = config.get();

        let raw_sender = meta
            .and_then(|m| m.get("sender_id").or_else(|| m.get("sender")))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        // 渠道优先看 sender_id 前缀：统一到 main 会话后，channel 参数可能不再带
        // feishu/qq，但当前消息的 sender_id 一定标明来源。
        let sender_kind = raw_sender.as_deref().and_then(|raw| {
            if raw.starts_with("feishu:") {
                Some("feishu")
            } else if raw.starts_with("qq:") {
                Some("qq")
            } else {
                None
            }
        });
        let channel_kind = if channel.starts_with("feishu") {
            Some("feishu")
        } else if channel.starts_with("qq") {
            Some("qq")
        } else {
            None
        };
        let prefix = sender_kind.or(channel_kind);

        let Some(prefix) = prefix else {
            let kind = if channel.starts_with("task_") {
                "scheduler"
            } else {
                "desktop"
            };
            return Self {
                channel: kind.to_string(),
                session_key: channel.to_string(),
                sender_id: None,
                is_owner: true,
            };
        };

        let sender_id = raw_sender.as_deref().map(|raw| {
            if raw.starts_with("feishu:") || raw.starts_with("qq:") {
                raw.to_string()
            } else {
                format!("{prefix}:{raw}")
            }
        });

        let owner_id = match prefix {
            "feishu" => cfg.feishu.owner_open_id.trim(),
            "qq" => cfg.qq.owner_open_id.trim(),
            _ => "",
        };
        let is_owner = match raw_sender.as_deref() {
            Some(raw) if !owner_id.is_empty() => {
                let bare = raw
                    .strip_prefix("feishu:")
                    .or_else(|| raw.strip_prefix("qq:"))
                    .unwrap_or(raw);
                bare == owner_id
            }
            _ => false,
        };

        Self {
            channel: prefix.to_string(),
            session_key: channel.to_string(),
            sender_id,
            is_owner,
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

    fn config_with_owners() -> ConfigStore {
        let mut cfg = crate::config::default_config();
        cfg.feishu.owner_open_id = "ou_owner".to_string();
        cfg.qq.owner_open_id = "qq_owner".to_string();
        ConfigStore::from_config(cfg)
    }

    #[test]
    fn sender_prefix_wins_when_channel_is_unified() {
        // 统一到 main 会话后 channel 不再带渠道名，仍应靠 sender_id 识别来源与权限
        let cfg = config_with_owners();
        let meta = serde_json::json!({"sender_id": "feishu:ou_owner"});
        let ctx = ToolContext::from_input("main", Some(&meta), &cfg);
        assert_eq!(ctx.channel, "feishu");
        assert_eq!(ctx.sender_id.as_deref(), Some("feishu:ou_owner"));
        assert!(ctx.is_owner);
    }

    #[test]
    fn non_owner_is_detected_from_sender_id() {
        let cfg = config_with_owners();
        let meta = serde_json::json!({"sender_id": "qq:someone_else"});
        let ctx = ToolContext::from_input("main", Some(&meta), &cfg);
        assert_eq!(ctx.channel, "qq");
        assert!(!ctx.is_owner);
    }

    #[test]
    fn bare_sender_gets_channel_prefix() {
        let cfg = config_with_owners();
        let meta = serde_json::json!({"sender": "ou_owner"});
        let ctx = ToolContext::from_input("feishu_unified", Some(&meta), &cfg);
        assert_eq!(ctx.sender_id.as_deref(), Some("feishu:ou_owner"));
        assert!(ctx.is_owner);
    }

    #[test]
    fn scheduler_and_desktop_have_no_sender() {
        let cfg = config_with_owners();
        let ctx = ToolContext::from_input("task_daily", None, &cfg);
        assert_eq!(ctx.channel, "scheduler");
        assert!(ctx.is_owner);

        let ctx = ToolContext::from_input("desktop", None, &cfg);
        assert_eq!(ctx.channel, "desktop");
        assert!(ctx.is_owner);
    }
}
