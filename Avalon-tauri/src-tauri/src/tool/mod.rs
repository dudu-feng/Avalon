// Tool 工具层模块
//
// 职责：智能体基础操作工具（文件增删改查、目录列举、终端命令）+ 记忆检索工具
// （search_session_memory，接入 vector::MemoryIndex 检索契约），统一分发。
// 定义 engine 层所需的 ToolRegistry 契约（依赖反转：engine 依赖本模块的 trait，
// tool 不依赖 engine 的实现；记忆检索依赖 vector 的 MemoryIndex trait 而非具体实现）。

#![allow(dead_code)] // tool 模块部分工具供未来 engine 引用，接入后逐步移除

pub mod fs_tools;
pub mod memory_tools;
pub mod registry;

use async_trait::async_trait;
use serde_json::Value;

/// 工具注册表契约：engine 通过此 trait 调用工具（不依赖具体实现）
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// 生成供 LLM 理解的格式化工具列表（拼进 system prompt 的工具说明文本）
    fn get_tool_list(&self) -> String;
    /// 生成 OpenAI 原生 `tools` 参数（function.name + description + parameters JSON Schema）
    fn get_tools_schema(&self) -> Vec<Value>;
    /// 按名字调用工具，统一返回字符串结果（含错误信息）
    async fn invoke_tool(&self, name: &str, arguments: &Value) -> String;
}

#[allow(unused_imports)] // ToolSet 供未来 engine 显式引用
pub use registry::ToolSet;
