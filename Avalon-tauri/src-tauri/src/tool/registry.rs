// 工具注册表：工具元数据集中管理 + 按名分发
//
// get_tools_schema 生成 OpenAI 原生 tools 参数（function.name + description + parameters JSON Schema）；
// invoke_tool 用 match 分发（5 个工具 O(1) 命中，替代 Python 线性遍历）。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::channel::contact::ContactStore;
use crate::channel::{FeishuHandle, QqHandle};
use crate::config::{ConfigStore, SearchMode};
use crate::prompt::PromptAssembler;
use crate::scheduler::TaskStore;
use crate::session::SessionStore;
use crate::skill::SkillRegistry;
use crate::soul::SoulRegistry;
use crate::vector::MemoryIndex;

use super::contact_tools;
use super::feishu_tools;
use super::fs_tools;
use super::history_tools;
use super::memory_tools;
use super::qq_tools;
use super::sandbox::Sandbox;
use super::scheduler_tools;
use super::skill_tools;
use super::soul_tools;
use super::vision_tools;
use super::web_tools::SearchClient;
use super::{tool_scope, ToolContext, ToolRegistry, ToolScope};
use super::schema::{
    contact_tool_defs, feishu_tool_defs, history_tool_defs, memory_tool_def, ocr_tool_def,
    qq_tool_defs, read_tool_result_def, scheduler_tool_defs, skill_tool_defs, soul_tool_defs,
    tool_defs, vision_tool_def, web_tool_defs, ToolDef,
};
use crate::vision::VisionService;

/// 工具注册表实现：基础文件/终端工具 + 可选记忆检索（注入 MemoryIndex 后启用）
pub struct ToolSet {
    /// 记忆检索后端（None 则不暴露 search_session_memory 工具）
    memory: Option<Arc<dyn MemoryIndex>>,
    /// 配置句柄（提供检索默认模式；None 时缺省 mode 兜底 hybrid）
    config: Option<ConfigStore>,
    /// 会话存储（None 则不暴露会话历史原文工具）
    session: Option<Arc<dyn SessionStore>>,
    /// 渠道通讯录（None 则不暴露通讯录工具）
    contacts: Option<Arc<ContactStore>>,
    /// 定时任务存储（None 则不暴露定时任务工具）
    scheduler: Option<Arc<TaskStore>>,
    /// 联网搜索客户端（None 则不暴露搜索工具）
    search: Option<SearchClient>,
    /// 飞书发送句柄（None 则不暴露飞书工具）
    feishu: Option<Arc<FeishuHandle>>,
    /// QQ 发送句柄（None 则不暴露 QQ 工具）
    qq: Option<Arc<QqHandle>>,
    /// 灵魂注册表（None 则不暴露灵魂条目工具）
    soul: Option<Arc<SoulRegistry>>,
    /// 技能注册表（None 则不暴露 use_skill 工具）
    skill: Option<Arc<SkillRegistry>>,
    /// 提示词组装器（条目变更后 refresh 缓存，下一轮对话生效）
    prompt: Option<Arc<PromptAssembler>>,
    /// 视觉服务（None 则不暴露 view_image 工具）
    vision: Option<Arc<VisionService>>,
    /// 直连 HTTP 客户端（install_skill 拉取技能正文用，独立于 SearchClient 的搜索服务）
    http: reqwest::Client,
}

impl ToolSet {
    pub fn new() -> Self {
        Self {
            memory: None,
            config: None,
            session: None,
            contacts: None,
            scheduler: None,
            search: None,
            feishu: None,
            qq: None,
            soul: None,
            skill: None,
            prompt: None,
            vision: None,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
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

    /// 注入会话存储，启用 list_session_history / read_session_history 工具（仅主人）
    pub fn with_session(mut self, session: Arc<dyn SessionStore>) -> Self {
        self.session = Some(session);
        self
    }

    /// 注入渠道通讯录，启用 record_user / update_user / get_user / list_users / get_owner_contact 工具
    pub fn with_contacts(mut self, contacts: Arc<ContactStore>) -> Self {
        self.contacts = Some(contacts);
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

    /// 注入 QQ 发送句柄，启用 qq_notify_owner / qq_send_to 工具。
    /// 与飞书一致：在线与否由句柄在调用时回答，不按配置开关静态隐藏
    pub fn with_qq(mut self, qq: Arc<QqHandle>) -> Self {
        self.qq = Some(qq);
        self
    }

    /// 注入灵魂注册表，启用 list/create/update/delete_soul_entry 工具
    pub fn with_soul(mut self, soul: Arc<SoulRegistry>) -> Self {
        self.soul = Some(soul);
        self
    }

    /// 注入技能注册表，启用 use_skill 工具（技能清单已由 PromptAssembler 注入 system prompt）
    pub fn with_skill(mut self, skill: Arc<SkillRegistry>) -> Self {
        self.skill = Some(skill);
        self
    }

    /// 注入提示词组装器，画像/灵魂更新成功后 refresh 缓存，下一轮对话生效
    pub fn with_prompt(mut self, prompt: Arc<PromptAssembler>) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// 注入视觉服务。是否暴露 view_image 由当前 `[vision].enabled` 决定，
    /// 支持设置页热开关：关掉后工具 schema 立即消失
    pub fn with_vision(mut self, vision: Arc<VisionService>) -> Self {
        self.vision = Some(vision);
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

    /// 当前暴露的全部工具定义（基础工具 + 可选记忆检索 + 通讯录）
    fn defs(&self, ctx: &ToolContext) -> Vec<ToolDef> {
        let mut defs = tool_defs(&self.sandbox());
        if self.memory.is_some() {
            defs.push(memory_tool_def());
        }
        if self.session.is_some() {
            // 先构建完整集合，再按调用方身份过滤：非主人白名单可能放行只读历史工具
            defs.extend(history_tool_defs(true));
            defs.push(read_tool_result_def());
        }
        if self.contacts.is_some() {
            defs.extend(contact_tool_defs(true));
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
        if self.qq.is_some() {
            defs.extend(qq_tool_defs());
        }
        if self.soul.is_some() {
            defs.extend(soul_tool_defs());
        }
        if self.skill.is_some() {
            defs.extend(skill_tool_defs());
        }
        // 视觉开关可在设置页随时关闭：已注入也按当前配置隐藏，避免模型反复试一个用不了的工具
        let vision_enabled = self
            .config
            .as_ref()
            .map(|c| c.get().vision.enabled)
            .unwrap_or(true);
        if self.vision.is_some() && vision_enabled {
            defs.push(vision_tool_def());
        }
        // OCR 工具只在引擎真正加载后暴露；引擎未实现时不把必然失败的工具交给模型
        if vision_enabled && self.vision.as_ref().is_some_and(|v| v.ocr_available()) {
            defs.push(ocr_tool_def());
        }

        if ctx.is_owner {
            return defs;
        }

        // 非主人：默认无工具；只放行配置白名单 ∩ NonOwnerSafe
        let allow = self
            .config
            .as_ref()
            .map(|c| c.get().tools.non_owner_allowlist.clone())
            .unwrap_or_default();
        defs.into_iter()
            .filter(|t| {
                tool_scope(t.name) == ToolScope::NonOwnerSafe
                    && allow.iter().any(|name| name == t.name)
            })
            .collect()
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
    fn get_tools_schema(&self, ctx: &ToolContext) -> Vec<Value> {
        self.defs(ctx)
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

    async fn invoke_tool(&self, ctx: &ToolContext, name: &str, args: &Value) -> String {
        // schema 已经按上下文隐藏，这里再兜一层：模型可能凭历史记忆硬调工具名。
        // 非主人默认一个都不放行；白名单也只对 NonOwnerSafe 生效。
        if !ctx.is_owner {
            let allow = self
                .config
                .as_ref()
                .map(|c| c.get().tools.non_owner_allowlist.clone())
                .unwrap_or_default();
            let permitted = tool_scope(name) == ToolScope::NonOwnerSafe
                && allow.iter().any(|allowed| allowed == name);
            if !permitted {
                return "权限不足: 该工具未对非主人开放".to_string();
            }
        }

        match name {
            "record_user" => match &self.contacts {
                Some(s) => contact_tools::record_user(args, s, ctx),
                None => "通讯录未配置".to_string(),
            },
            "update_user" => match &self.contacts {
                Some(s) => contact_tools::update_user(args, s, ctx),
                None => "通讯录未配置".to_string(),
            },
            "get_user" => match &self.contacts {
                Some(s) => contact_tools::get_user(args, s, ctx),
                None => "通讯录未配置".to_string(),
            },
            "list_users" => match &self.contacts {
                Some(s) => contact_tools::list_users(args, s, ctx),
                None => "通讯录未配置".to_string(),
            },
            "get_owner_contact" => match &self.contacts {
                Some(s) => contact_tools::get_owner_contact(s, ctx),
                None => "通讯录未配置".to_string(),
            },
            "read_file" => fs_tools::read_file(args, &self.sandbox()),
            "write_file" => fs_tools::write_file(args, &self.sandbox()),
            "delete_file" => fs_tools::delete_file(args, &self.sandbox()),
            "run_shell_command" => fs_tools::run_shell_command(args, &self.sandbox()).await,
            "get_directory_contents" => fs_tools::get_directory_contents(args, &self.sandbox()),
            "view_image" => match &self.vision {
                Some(v) => {
                    vision_tools::view_image(args, v, &self.sandbox(), self.config.as_ref()).await
                }
                None => "图片查看未启用（设置 → 视觉）".to_string(),
            },
            "ocr_image" => match &self.vision {
                Some(v) => {
                    vision_tools::ocr_image(args, v, &self.sandbox(), self.config.as_ref()).await
                }
                None => "OCR 未启用（设置 → 视觉 → OCR）".to_string(),
            },
            "search_session_memory" => match &self.memory {
                Some(m) => memory_tools::search_session_memory(
                    args,
                    m.as_ref(),
                    self.default_search_mode(),
                ),
                None => "记忆检索未配置".to_string(),
            },
            "list_session_history" => match &self.session {
                Some(s) => history_tools::list_session_history(args, s.as_ref(), ctx),
                None => "会话历史未配置".to_string(),
            },
            "read_session_history" => match &self.session {
                Some(s) => history_tools::read_session_history(args, s.as_ref(), ctx),
                None => "会话历史未配置".to_string(),
            },
            "read_tool_result" => match &self.session {
                Some(s) => history_tools::read_tool_result(args, s.as_ref()),
                None => "会话历史未配置".to_string(),
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
                Some(h) => feishu_tools::notify_owner(args, h, self.contacts.as_deref()).await,
                None => "飞书工具未配置".to_string(),
            },
            "feishu_send_to" => match &self.feishu {
                Some(h) => feishu_tools::send_to(args, h).await,
                None => "飞书工具未配置".to_string(),
            },
            "qq_notify_owner" => match &self.qq {
                Some(h) => qq_tools::notify_owner(args, h, self.contacts.as_deref()).await,
                None => "QQ 工具未配置".to_string(),
            },
            "qq_send_to" => match &self.qq {
                Some(h) => qq_tools::send_to(args, h).await,
                None => "QQ 工具未配置".to_string(),
            },
            "list_soul_entries" => match &self.soul {
                Some(s) => soul_tools::list_soul_entries(s),
                None => "灵魂注册表未配置".to_string(),
            },
            "use_skill" => match &self.skill {
                Some(s) => skill_tools::use_skill(args, s),
                None => "技能未配置".to_string(),
            },
            "create_skill" => match &self.skill {
                Some(s) => skill_tools::create_skill(args, s),
                None => "技能未配置".to_string(),
            },
            "update_skill" => match &self.skill {
                Some(s) => skill_tools::update_skill(args, s),
                None => "技能未配置".to_string(),
            },
            "delete_skill" => match &self.skill {
                Some(s) => skill_tools::delete_skill(args, s),
                None => "技能未配置".to_string(),
            },
            "install_skill" => match &self.skill {
                Some(s) => skill_tools::install_skill(args, s, &self.http).await,
                None => "技能未配置".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_names(set: &ToolSet, ctx: &ToolContext) -> Vec<String> {
        set.get_tools_schema(ctx)
            .into_iter()
            .filter_map(|t| {
                t.pointer("/function/name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn 注入句柄后暴露对应渠道工具() {
        let set = ToolSet::new()
            .with_feishu(Arc::new(FeishuHandle::new()))
            .with_qq(Arc::new(QqHandle::new()));
        let names = tool_names(&set, &ToolContext::owner_desktop());
        assert!(names.contains(&"feishu_notify_owner".to_string()));
        assert!(names.contains(&"feishu_send_to".to_string()));
        assert!(names.contains(&"qq_notify_owner".to_string()));
        assert!(names.contains(&"qq_send_to".to_string()));
        // 主人信息改由通讯录 get_owner_contact 提供，渠道侧不再暴露查询工具
        assert!(!names.contains(&"feishu_get_owner".to_string()));
        assert!(!names.contains(&"qq_get_owner".to_string()));
    }

    #[test]
    fn 渠道发送工具都支持可选引用() {
        let set = ToolSet::new()
            .with_feishu(Arc::new(FeishuHandle::new()))
            .with_qq(Arc::new(QqHandle::new()));
        let schema = set.get_tools_schema(&ToolContext::owner_desktop());
        for name in ["feishu_send_to", "qq_send_to"] {
            let tool = schema
                .iter()
                .find(|t| t.pointer("/function/name").and_then(Value::as_str) == Some(name))
                .unwrap_or_else(|| panic!("{name} 应存在"));
            assert!(
                tool.pointer("/function/parameters/properties/quote_message_id")
                    .is_some(),
                "{name} 应提供 quote_message_id"
            );
            let required = tool
                .pointer("/function/parameters/required")
                .and_then(Value::as_array)
                .expect("required 应为数组");
            assert!(
                !required.iter().any(|v| v == "quote_message_id"),
                "{name} 的 quote_message_id 应为可选"
            );
        }
    }

    #[test]
    fn 未注入句柄时不暴露渠道工具() {
        let names = tool_names(&ToolSet::new(), &ToolContext::owner_desktop());
        assert!(!names.iter().any(|n| n.starts_with("feishu_")));
        assert!(!names.iter().any(|n| n.starts_with("qq_")));
    }

    #[test]
    fn 非主人默认没有任何工具() {
        let set = ToolSet::new()
            .with_config(ConfigStore::from_config(crate::config::default_config()))
            .with_contacts(Arc::new(ContactStore::new(
                std::env::temp_dir().join("avalon-contact-registry-test.json"),
            )))
            .with_feishu(Arc::new(FeishuHandle::new()))
            .with_qq(Arc::new(QqHandle::new()));
        let ctx = ToolContext {
            channel: "feishu".to_string(),
            session_key: "feishu_unified".to_string(),
            sender_id: Some("feishu:ou_guest".to_string()),
            is_owner: false,
        };
        assert!(tool_names(&set, &ctx).is_empty());
    }

    #[test]
    fn 非主人白名单只放行安全工具() {
        let mut cfg = crate::config::default_config();
        cfg.tools.non_owner_allowlist = vec![
            "web_search".to_string(),
            "read_file".to_string(),
            "list_session_history".to_string(),
        ];
        let set = ToolSet::new()
            .with_config(ConfigStore::from_config(cfg.clone()))
            .with_search(SearchClient::new(cfg.search));
        let ctx = ToolContext {
            channel: "feishu".to_string(),
            session_key: "feishu_unified".to_string(),
            sender_id: Some("feishu:ou_guest".to_string()),
            is_owner: false,
        };
        let names = tool_names(&set, &ctx);
        assert!(names.contains(&"web_search".to_string()));
        // 危险工具即使写进白名单也不生效
        assert!(!names.contains(&"read_file".to_string()));
    }

    #[test]
    fn 会话历史工具定义始终完整构建() {
        assert!(history_tool_defs(false).is_empty());
        let names: Vec<&str> = history_tool_defs(true).iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["list_session_history", "read_session_history"]);
        assert_eq!(tool_scope("list_session_history"), ToolScope::NonOwnerSafe);
        assert_eq!(tool_scope("read_session_history"), ToolScope::NonOwnerSafe);
    }

    #[test]
    fn 主人可见通讯录全部工具() {
        let set = ToolSet::new()
            .with_config(ConfigStore::from_config(crate::config::default_config()))
            .with_contacts(Arc::new(ContactStore::new(
                std::env::temp_dir().join("avalon-contact-registry-owner-test.json"),
            )));
        let names = tool_names(&set, &ToolContext::owner_desktop());
        assert!(names.contains(&"record_user".to_string()));
        assert!(names.contains(&"update_user".to_string()));
        assert!(names.contains(&"get_user".to_string()));
        assert!(names.contains(&"list_users".to_string()));
        assert!(names.contains(&"get_owner_contact".to_string()));
    }
}
