// 工具注册表：工具元数据集中管理 + 按名分发
//
// get_tool_list 按注册顺序拼格式化列表（供 LLM 理解可用工具）；
// get_tools_schema 生成 OpenAI 原生 tools 参数（function.name + description + parameters JSON Schema）；
// invoke_tool 用 match 分发（5 个工具 O(1) 命中，替代 Python 线性遍历）。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::{ConfigStore, SearchMode};
use crate::vector::MemoryIndex;

use super::fs_tools;
use super::memory_tools;
use super::ToolRegistry;

/// 工具元数据（名字 + 描述 + 参数 JSON Schema，供 get_tool_list / get_tools_schema 复用）
struct ToolDef {
    name: &'static str,
    description: &'static str,
    /// OpenAI function.parameters（JSON Schema），参数从 fs_tools/memory_tools 的 args.get(...) 反推
    parameters: Value,
}

/// 基础文件/终端工具定义（5 个，固定注册）
fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file",
            description: "读取指定文件内容",
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "文件路径"}
                },
                "required": ["file_path"]
            }),
        },
        ToolDef {
            name: "write_file",
            description: "创建或覆盖写入文件",
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "文件路径"},
                    "content": {"type": "string", "description": "文件内容"}
                },
                "required": ["file_path", "content"]
            }),
        },
        ToolDef {
            name: "delete_file",
            description: "删除指定文件",
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "文件路径"}
                },
                "required": ["file_path"]
            }),
        },
        ToolDef {
            name: "run_shell_command",
            description: "在终端执行命令并返回标准输出与错误",
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "命令字符串"}
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "get_directory_contents",
            description: "获取目录下的文件和子目录",
            parameters: json!({
                "type": "object",
                "properties": {
                    "directory_path": {"type": "string", "description": "目录路径"}
                },
                "required": ["directory_path"]
            }),
        },
    ]
}

/// 记忆检索工具定义（仅注入 memory 后端时暴露）
fn memory_tool_def() -> ToolDef {
    ToolDef {
        name: "search_session_memory",
        description: "查询历史会话记忆",
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "检索关键词/问题"},
                "mode": {
                    "type": "string",
                    "enum": ["semantic", "keyword", "hybrid"],
                    "description": "检索模式（可选，缺省取配置）"
                },
                "topk": {"type": "integer", "description": "返回条数（可选，默认 5）"},
                "time_range": {
                    "type": "string",
                    "description": "时间范围 YYYY-MM-DD 或 YYYY-MM-DD,YYYY-MM-DD（可选）"
                }
            },
            "required": ["query"]
        }),
    }
}

/// 工具注册表实现：基础文件/终端工具 + 可选记忆检索（注入 MemoryIndex 后启用）
pub struct ToolSet {
    /// 记忆检索后端（None 则不暴露 search_session_memory 工具）
    memory: Option<Arc<dyn MemoryIndex>>,
    /// 配置句柄（提供检索默认模式；None 时缺省 mode 兜底 hybrid）
    config: Option<ConfigStore>,
}

impl ToolSet {
    pub fn new() -> Self {
        Self {
            memory: None,
            config: None,
        }
    }

    /// 注入记忆检索后端，启用 search_session_memory 工具
    pub fn with_memory(mut self, memory: Arc<dyn MemoryIndex>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// 注入配置句柄，缺省检索模式跟随 config.session_memory.search_mode（支持热更新）
    pub fn with_config(mut self, config: ConfigStore) -> Self {
        self.config = Some(config);
        self
    }

    /// 记忆检索的缺省模式：优先配置值，无配置兜底 hybrid
    fn default_search_mode(&self) -> SearchMode {
        self.config
            .as_ref()
            .map(|c| c.get().session_memory.search_mode)
            .unwrap_or(SearchMode::Hybrid)
    }

    /// 当前暴露的全部工具定义（基础工具 + 可选记忆检索）
    fn defs(&self) -> Vec<ToolDef> {
        let mut defs = tool_defs();
        if self.memory.is_some() {
            defs.push(memory_tool_def());
        }
        defs
    }
}

#[async_trait]
impl ToolRegistry for ToolSet {
    fn get_tool_list(&self) -> String {
        let mut out = String::from("## 可用工具列表\n");
        for t in self.defs() {
            out.push_str(&format!("- **{}**: {}\n", t.name, t.description));
        }
        out.trim_end().to_string()
    }

    fn get_tools_schema(&self) -> Vec<Value> {
        self.defs()
            .into_iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect()
    }

    async fn invoke_tool(&self, name: &str, args: &Value) -> String {
        match name {
            "read_file" => fs_tools::read_file(args),
            "write_file" => fs_tools::write_file(args),
            "delete_file" => fs_tools::delete_file(args),
            "run_shell_command" => fs_tools::run_shell_command(args).await,
            "get_directory_contents" => fs_tools::get_directory_contents(args),
            "search_session_memory" => match &self.memory {
                Some(m) => memory_tools::search_session_memory(args, m.as_ref(), self.default_search_mode()),
                None => "记忆检索未配置".to_string(),
            },
            _ => format!("未找到工具: {name}"),
        }
    }
}
