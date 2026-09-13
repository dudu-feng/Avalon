// 工具注册表：工具元数据集中管理 + 按名分发
//
// get_tools_schema 生成 OpenAI 原生 tools 参数（function.name + description + parameters JSON Schema）；
// invoke_tool 用 match 分发（5 个工具 O(1) 命中，替代 Python 线性遍历）。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::channel::FeishuHandle;
use crate::config::{ConfigStore, SearchMode};
use crate::prompt::PromptAssembler;
use crate::scheduler::TaskStore;
use crate::soul::SoulRegistry;
use crate::vector::MemoryIndex;

use super::feishu_tools;
use super::fs_tools;
use super::memory_tools;
use super::sandbox::Sandbox;
use super::scheduler_tools;
use super::soul_tools;
use super::web_tools::SearchClient;
use super::ToolRegistry;

/// 工具元数据（名字 + 描述 + 参数 JSON Schema，供 get_tools_schema 使用）
struct ToolDef {
    name: &'static str,
    /// 用 String 而非 &'static str：文件与终端工具的描述要把当前的工作区
    /// 和可用命令写进去。让模型一开始就知道边界，比让它撞一次拒绝再重试省一轮
    description: String,
    /// OpenAI function.parameters（JSON Schema），参数从 fs_tools/memory_tools 的 args.get(...) 反推
    parameters: Value,
}

/// 基础文件/终端工具定义（5 个，固定注册）。
/// 描述随沙箱配置变化 —— 用户改了工作区，模型下一轮看到的就是新边界
fn tool_defs(sb: &Sandbox) -> Vec<ToolDef> {
    let scope = format!("只能访问工作区内的路径，当前工作区：{}", sb.roots_hint());
    vec![
        ToolDef {
            name: "read_file",
            description: format!("读取指定文件内容。{scope}"),
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
            description: format!("创建或覆盖写入文件，父目录会自动创建。{scope}"),
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
            description: format!("删除指定文件。{scope}"),
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
            // 「不经过 shell」必须写进描述里。否则模型会按惯例拼 `a && b`、
            // 用 > 重定向，然后对着一个「参数被当成字面量」的怪结果反复试
            description: format!(
                "执行一条系统命令并返回输出。命令不经过 shell 解释：参数必须逐个放进 args 数组，\
                 && || | > < ; 等符号不起作用，无法使用管道、重定向和命令拼接。\
                 需要读写文件请用 read_file / write_file。当前允许的命令：{}",
                sb.allowlist_hint()
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "命令名本身，不带路径、不带参数，例如 ping"},
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "参数列表，每个参数一项，例如 [\"-n\", \"1\", \"127.0.0.1\"]"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "get_directory_contents",
            description: format!("获取目录下的文件和子目录。{scope}"),
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
        description: "查询历史会话记忆".to_string(),
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
            description: "创建一个定时任务（在指定时间自动执行一次对话）。schedule_type 为 once/daily/weekly；once 的 schedule_value 形如 'YYYY-MM-DD HH:MM'，daily 为 'HH:MM'，weekly 为 'N HH:MM'（N=1 周一 .. 7 周日）".to_string(),
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
            description: "列出全部定时任务（id、来源、触发时间、内容、是否启用），供创建前查重".to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: "delete_scheduled_task",
            description: "删除指定 id 的定时任务".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "任务 id"}
                },
                "required": ["task_id"]
            }),
        },
        ToolDef {
            name: "update_scheduled_task",
            description: "编辑指定 id 的定时任务（名称、内容或触发时间）。task_id 用 list_scheduled_tasks 查得；schedule_type/schedule_value 格式同 create_scheduled_task".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "description": "任务 id"},
                    "name": {"type": "string", "description": "任务名称（简短标题）"},
                    "prompt": {"type": "string", "description": "任务内容（每次触发时喂给 agent 的输入）"},
                    "schedule_type": {"type": "string", "enum": ["once", "daily", "weekly"], "description": "触发方式"},
                    "schedule_value": {"type": "string", "description": "触发时间：once=YYYY-MM-DD HH:MM；daily=HH:MM；weekly=N HH:MM"}
                },
                "required": ["task_id", "name", "prompt", "schedule_type", "schedule_value"]
            }),
        },
    ]
}

/// 灵魂注册表工具定义（仅注入 SoulRegistry 时暴露，4 个）
fn soul_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_soul_entries",
            description: "列出灵魂注册表中的可见条目（id、类别、保护级别、标题，不含系统基本设定），\
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

/// 联网搜索工具定义（仅注入 SearchClient 时暴露，2 个）
fn web_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "web_search",
            // 明确分工：这里只给摘要，要正文得再调 read_web_page。
            // 不写清楚的话模型容易拿着一句摘要就下结论
            description: "搜索互联网获取实时信息。返回若干条结果的标题、链接与摘要；\
                          需要某条结果的完整内容时，再用 read_web_page 打开它的链接"
                .to_string(),
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
                         PDF、图片、音视频等二进制格式无法读取。正文过长会被截断，\
                         截断时会在结尾提示下一个 offset，带上 offset 参数继续读下一段"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "网页链接，必须是 http 或 https"},
                    "offset": {"type": "integer", "description": "续读起始字符位置（可选，默认 0 从头读）"}
                },
                "required": ["url"]
            }),
        },
    ]
}

/// 飞书发送工具定义（仅注入 FeishuHandle 时暴露，2 个）
fn feishu_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "feishu_notify_owner",
            // 「主动」两个字是重点：这是定时任务唯一能触达用户的出口
            description: "通过飞书主动给主人发一条消息。用于定时任务的结果推送、\
                          需要提醒用户的重要发现。收件人由配置决定，无需也无法指定。\
                          注意：正在进行的飞书对话，回复由系统自动发送，不要用本工具重发"
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
                          注意：正在进行的飞书对话，回复由系统自动发送，\
                          本工具只用于发往其它会话或额外追加一条"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {"type": "string", "description": "目标会话 id，oc_ 开头"},
                    "text": {"type": "string", "description": "消息正文，支持 markdown"}
                },
                "required": ["chat_id", "text"]
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
    /// 飞书发送句柄（None 则不暴露飞书工具）
    feishu: Option<Arc<FeishuHandle>>,
    /// 灵魂注册表（None 则不暴露灵魂条目工具）
    soul: Option<Arc<SoulRegistry>>,
    /// 提示词组装器（条目变更后 refresh 缓存，下一轮对话生效）
    prompt: Option<Arc<PromptAssembler>>,
}

impl ToolSet {
    pub fn new() -> Self {
        Self {
            memory: None,
            config: None,
            scheduler: None,
            search: None,
            feishu: None,
            soul: None,
            prompt: None,
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

    /// 注入飞书发送句柄，启用 feishu_notify_owner / feishu_send_to 工具。
    ///
    /// 与搜索工具不同，这里不按配置开关判断：渠道能在运行期随时启停，
    /// 启动那一刻的 enabled 说明不了运行期的状态。在线与否由句柄在调用时回答。
    pub fn with_feishu(mut self, feishu: Arc<FeishuHandle>) -> Self {
        self.feishu = Some(feishu);
        self
    }

    /// 注入灵魂注册表，启用 list/create/update/delete_soul_entry 工具
    pub fn with_soul(mut self, soul: Arc<SoulRegistry>) -> Self {
        self.soul = Some(soul);
        self
    }

    /// 注入提示词组装器，画像/灵魂更新成功后 refresh 缓存，下一轮对话生效
    pub fn with_prompt(mut self, prompt: Arc<PromptAssembler>) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// 记忆检索的缺省模式：优先配置值，无配置兜底 hybrid
    fn default_search_mode(&self) -> SearchMode {
        self.config
            .as_ref()
            .map(|c| c.get().session_memory.search_mode)
            .unwrap_or(SearchMode::Hybrid)
    }

    /// 现读配置建沙箱。
    ///
    /// 每次调用重建而不是在构造时定一次，换来配置热更新 —— 用户在设置页
    /// 改了工作区，下一次工具调用立刻生效，不用重启。代价是几个 canonicalize
    /// 系统调用，相对于一次 LLM 往返可以忽略。
    ///
    /// 没有配置句柄时返回空沙箱（拒绝一切）。这是刻意的失败方向：
    /// 拿不到边界定义时应当什么都不许，而不是什么都放行
    fn sandbox(&self) -> Sandbox {
        match &self.config {
            Some(c) => Sandbox::from_config(&c.get()),
            None => Sandbox::deny_all(),
        }
    }

    /// 当前暴露的全部工具定义（基础工具 + 可选记忆检索）
    fn defs(&self) -> Vec<ToolDef> {
        let mut defs = tool_defs(&self.sandbox());
        if self.memory.is_some() {
            defs.push(memory_tool_def());
        }
        if self.scheduler.is_some() {
            defs.extend(scheduler_tool_defs());
        }
        if self.search.is_some() {
            defs.extend(web_tool_defs());
        }
        if self.feishu.is_some() {
            defs.extend(feishu_tool_defs());
        }
        if self.soul.is_some() {
            defs.extend(soul_tool_defs());
        }
        defs
    }

    /// 调用灵魂条目变更工具（create/update/delete），成功后刷新提示词缓存（下一轮对话即生效）。
    /// 错误（参数错误 / 灵魂条目失败）不刷新 —— 没改动就不该刷缓存。
    fn invoke_soul_mutation(&self, f: impl Fn(&SoulRegistry) -> String) -> String {
        let Some(soul) = &self.soul else {
            return "灵魂注册表未配置".to_string();
        };
        let out = f(soul);
        if !out.starts_with("参数错误") && !out.starts_with("灵魂条目") {
            if let Some(asm) = &self.prompt {
                asm.refresh();
            }
        }
        out
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
            "read_file" => fs_tools::read_file(args, &self.sandbox()),
            "write_file" => fs_tools::write_file(args, &self.sandbox()),
            "delete_file" => fs_tools::delete_file(args, &self.sandbox()),
            "run_shell_command" => fs_tools::run_shell_command(args, &self.sandbox()).await,
            "get_directory_contents" => fs_tools::get_directory_contents(args, &self.sandbox()),
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
            "update_scheduled_task" => match &self.scheduler {
                Some(s) => scheduler_tools::update_scheduled_task(args, s),
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
            "feishu_notify_owner" => match &self.feishu {
                Some(h) => feishu_tools::notify_owner(args, h, self.config.as_ref()).await,
                None => "飞书工具未配置".to_string(),
            },
            "feishu_send_to" => match &self.feishu {
                Some(h) => feishu_tools::send_to(args, h).await,
                None => "飞书工具未配置".to_string(),
            },
            "list_soul_entries" => match &self.soul {
                Some(s) => soul_tools::list_soul_entries(s),
                None => "灵魂注册表未配置".to_string(),
            },
            "get_soul_entry" => match &self.soul {
                Some(s) => soul_tools::get_soul_entry(args, s),
                None => "灵魂注册表未配置".to_string(),
            },
            "create_soul_entry" => {
                self.invoke_soul_mutation(|s| soul_tools::create_soul_entry(args, s))
            }
            "update_soul_entry" => {
                self.invoke_soul_mutation(|s| soul_tools::update_soul_entry(args, s))
            }
            "delete_soul_entry" => {
                self.invoke_soul_mutation(|s| soul_tools::delete_soul_entry(args, s))
            }
            "set_soul_entry_enabled" => {
                self.invoke_soul_mutation(|s| soul_tools::set_soul_entry_enabled(args, s))
            }
            _ => format!("未找到工具: {name}"),
        }
    }
}
