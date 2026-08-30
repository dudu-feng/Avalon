// 工具注册表：工具元数据集中管理 + 按名分发
//
// get_tools_schema 生成 OpenAI 原生 tools 参数（function.name + description + parameters JSON Schema）；
// invoke_tool 用 match 分发（5 个工具 O(1) 命中，替代 Python 线性遍历）。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::{ConfigStore, SearchMode};
use crate::scheduler::TaskStore;
use crate::vector::MemoryIndex;

use super::fs_tools;
use super::memory_tools;
use super::scheduler_tools;
use super::web_tools::SearchClient;
use super::ToolRegistry;

/// 工具元数据（名字 + 描述 + 参数 JSON Schema，供 get_tools_schema 使用）
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

/// 定时任务工具定义（仅注入 TaskStore 时暴露，3 个）
fn scheduler_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "create_scheduled_task",
            description: "创建一个定时任务（在指定时间自动执行一次对话）。schedule_type 为 once/daily/weekly；once 的 schedule_value 形如 'YYYY-MM-DD HH:MM'，daily 为 'HH:MM'，weekly 为 'N HH:MM'（N=1 周一 .. 7 周日）",
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "任务名称（简短标题）"},
                    "prompt": {"type": "string", "description": "任务内容（每次触发时喂给 agent 的输入）"},
                    "schedule_type": {"type": "string", "enum": ["once", "daily", "weekly"], "description": "触发方式"},
                    "schedule_value": {"type": "string", "description": "触发时间：once=YYYY-MM-DD HH:MM；daily=HH:MM；weekly=N HH:MM"}
                },
                "required": ["name", "prompt", "schedule_type", "schedule_value"]
            }),
        },
        ToolDef {
            name: "list_scheduled_tasks",
            description: "列出全部定时任务（id、来源、触发时间、内容、是否启用），供创建前查重",
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: "delete_scheduled_task",
            description: "删除指定 id 的定时任务",
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "任务 id"}
                },
                "required": ["task_id"]
            }),
        },
    ]
}

/// 联网搜索工具定义（仅注入 SearchClient 时暴露，2 个）
fn web_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "web_search",
            // 明确分工：这里只给摘要，要正文得再调 read_web_page。
            // 不写清楚的话模型容易拿着一句摘要就下结论
            description: "搜索互联网获取实时信息。返回若干条结果的标题、链接与摘要；\
                          需要某条结果的完整内容时，再用 read_web_page 打开它的链接",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词"},
                    "max_results": {"type": "integer", "description": "返回条数（可选，1-10，缺省取配置值）"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "read_web_page",
            description: "读取指定网页的正文并转为 Markdown。只支持 http/https 网页，\
                          PDF、图片、音视频等二进制格式无法读取。正文过长会被截断",
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "网页链接，必须是 http 或 https"}
                },
                "required": ["url"]
            }),
        },
    ]
}

/// 工具注册表实现：基础文件/终端工具 + 可选记忆检索（注入 MemoryIndex 后启用）
pub struct ToolSet {
    /// 记忆检索后端（None 则不暴露 search_session_memory 工具）
    memory: Option<Arc<dyn MemoryIndex>>,
    /// 配置句柄（提供检索默认模式；None 时缺省 mode 兜底 hybrid）
    config: Option<ConfigStore>,
    /// 定时任务存储（None 则不暴露定时任务工具）
    scheduler: Option<Arc<TaskStore>>,
    /// 联网搜索客户端（None 则不暴露搜索工具）
    search: Option<SearchClient>,
}

impl ToolSet {
    pub fn new() -> Self {
        Self {
            memory: None,
            config: None,
            scheduler: None,
            search: None,
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

    /// 注入定时任务存储，启用 create/list/delete_scheduled_task 工具
    pub fn with_scheduler(mut self, scheduler: Arc<TaskStore>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    /// 注入搜索客户端，启用 web_search / read_web_page 工具。
    /// 调用方负责判断配置是否开启 —— 不注入就等于对模型完全隐藏这两个工具
    pub fn with_search(mut self, search: SearchClient) -> Self {
        self.search = Some(search);
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
        if self.scheduler.is_some() {
            defs.extend(scheduler_tool_defs());
        }
        if self.search.is_some() {
            defs.extend(web_tool_defs());
        }
        defs
    }
}

#[async_trait]
impl ToolRegistry for ToolSet {
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
            "create_scheduled_task" => match &self.scheduler {
                Some(s) => scheduler_tools::create_scheduled_task(args, s),
                None => "定时任务未配置".to_string(),
            },
            "list_scheduled_tasks" => match &self.scheduler {
                Some(s) => scheduler_tools::list_scheduled_tasks(args, s),
                None => "定时任务未配置".to_string(),
            },
            "delete_scheduled_task" => match &self.scheduler {
                Some(s) => scheduler_tools::delete_scheduled_task(args, s),
                None => "定时任务未配置".to_string(),
            },
            "web_search" => match &self.search {
                Some(s) => s.search(args).await,
                None => "联网搜索未启用（配置 [search] enabled = true 后重启）".to_string(),
            },
            "read_web_page" => match &self.search {
                Some(s) => s.extract(args).await,
                None => "联网搜索未启用（配置 [search] enabled = true 后重启）".to_string(),
            },
            _ => format!("未找到工具: {name}"),
        }
    }
}
