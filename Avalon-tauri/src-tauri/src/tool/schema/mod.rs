// 工具 schema 定义：按域拆分，ToolDef 只在本模块内构造。

use serde_json::Value;

mod fs;
mod memory;
mod history;
mod contact;
mod scheduler;
mod skill;
mod soul;
mod web;
mod channel;
mod vision;

pub(crate) use fs::{tool_defs};
pub(crate) use memory::{memory_tool_def};
pub(crate) use history::{history_tool_defs, read_tool_result_def};
pub(crate) use contact::{contact_tool_defs};
pub(crate) use scheduler::{scheduler_tool_defs};
pub(crate) use skill::{skill_tool_defs};
pub(crate) use soul::{soul_tool_defs};
pub(crate) use web::{web_tool_defs};
pub(crate) use channel::{feishu_tool_defs, qq_tool_defs};
pub(crate) use vision::{vision_tool_def, ocr_tool_def};

/// 工具元数据（名字 + 描述 + 参数 JSON Schema，供 get_tools_schema 使用）
pub(crate) struct ToolDef {
    pub(crate) name: &'static str,
    /// 用 String 而非 &'static str：文件与终端工具的描述要把当前的工作区
    /// 和可用命令写进去。让模型一开始就知道边界，比让它撞一次拒绝再重试省一轮
    pub(crate) description: String,
    /// OpenAI function.parameters（JSON Schema），参数从 fs_tools/memory_tools 的 args.get(...) 反推
    pub(crate) parameters: Value,
}
