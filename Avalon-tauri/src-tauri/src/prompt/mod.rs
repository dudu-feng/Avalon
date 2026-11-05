// Prompt 提示词组装模块
//
// 职责：对话层 system prompt 的组装 —— 从灵魂注册表（SoulRegistry）读条目
// + 技能清单（SkillRegistry）+ 会话上下文。工具走 OpenAI 原生 tools 参数传入，不再写进 system prompt。
// 依赖反转：prompt 依赖 soul/skill 的注册表（Arc 共享），tool/session 以 &str 参数注入。
// action/compress 提示词模板仍硬编码在 llm/client.rs 与 templates.rs（协议耦合）。

#![allow(dead_code)] // prompt 模块供未来 engine 引用，当前无调用方，接入后移除

pub mod templates;

use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::skill::{Skill, SkillRegistry};
use crate::soul::SoulRegistry;

pub use templates::{build_compress_prompt, build_rolling_prompt};

/// 提示词组装器：从灵魂注册表读条目 + 技能清单拼 system prompt，带线程安全缓存
pub struct PromptAssembler {
    registry: Arc<SoulRegistry>,
    /// 技能注册表（None = 不注入技能清单）
    skills: Option<Arc<SkillRegistry>>,
    /// 是否注入通讯录使用规则（通讯录工具未启用时保持关闭）
    contacts: bool,
    /// None = 尚未加载；Some = 已缓存的注入片段列表（灵魂条目，不含动态技能清单）
    cache: RwLock<Option<Vec<String>>>,
}

impl PromptAssembler {
    /// 从灵魂注册表构造
    pub fn new(registry: Arc<SoulRegistry>) -> Self {
        Self {
            registry,
            skills: None,
            contacts: false,
            cache: RwLock::new(None),
        }
    }

    /// 注入技能注册表，启用「可用技能」清单注入
    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// 启用通讯录使用规则（与 ToolSet::with_contacts 同步开启）
    pub fn with_contacts_guidance(mut self) -> Self {
        self.contacts = true;
        self
    }

    /// 加载注入片段（灵魂注册表条目渲染后的列表），带缓存。
    /// 注册表文件不存在时 assemble 返回空列表，不报错（基本设定在初始化时已写入）。
    pub fn load_files(&self) -> Result<Vec<String>> {
        if let Some(cached) = self.cache.read().unwrap().as_ref() {
            return Ok(cached.clone());
        }

        let files = self.registry.assemble()?;

        *self.cache.write().unwrap() = Some(files.clone());
        Ok(files)
    }

    /// 清空缓存（灵魂条目变更后调用，下次 load_files 惰性重载）
    pub fn refresh(&self) {
        *self.cache.write().unwrap() = None;
    }

    /// 对话层稳定部分：灵魂条目 + 技能清单（动态，不缓存）+ 通讯录规则。
    ///
    /// 历史上限、压缩摘要、当前渠道等动态内容不属于这里：历史走原生 messages，
    /// 长期记忆由 session 层单独渲染，当前来源放在 user 正文前缀。
    pub fn assemble_stable_prompt(&self) -> Result<String> {
        let mut files = self.load_files()?;
        if let Some(skills) = &self.skills {
            if let Ok(list) = skills.list() {
                if !list.is_empty() {
                    files.push(render_skills_section(&list));
                }
            }
        }
        if self.contacts {
            files.push(render_contacts_section());
        }
        Ok(assemble_stable_system_prompt(&files))
    }

}

/// 渲染「可用技能」清单段（只列 name + description，正文用 use_skill 按需加载）
fn render_skills_section(skills: &[Skill]) -> String {
    let mut s = String::from("=====可用技能（需要时用 use_skill 工具加载正文）=====\n");
    for sk in skills {
        s.push_str(&format!("- {}: {}\n", sk.name, sk.description));
    }
    s.trim_end().to_string()
}

/// 渲染通讯录使用规则。通讯录是智能体主动维护的长期认知，不是消息日志
fn render_contacts_section() -> String {
    [
        "=====通讯录使用规则=====",
        "- 通讯录只记录长期认识的用户，字段只有 id、name、画像；用户说过的话在会话历史里，不重复记。",
        "- 外部渠道消息的来源标记里有 sender_id（如 feishu:ou_xxx / qq:xxx），可直接作为该用户的规范 id。",
        "- 第一次遇到值得长期记住的用户，调用 record_user；已经有记录就调用 update_user 补充或修正。",
        "- 需要确认某个用户是谁时，先 get_user；没有记录就承认不认识，不要编造身份。",
        "- 只有对方明确透露身份、偏好、关注点，或纠正先前信息时才更新；不要每轮都写，不记寒暄和一次性内容。",
        "- 画像只写对后续协作有用的信息；不记录密码、密钥、证件号等敏感内容，也不推断敏感身份。",
        "- profile 是整体替换：更新前先 get_user，保留仍然有效的旧内容。",
        "- 除非主人明确要求，不要主动罗列通讯录或他人的画像。",
    ]
    .join("\n")
}

/// 纯函数：文件列表 → 稳定 system prompt（不含历史；工具走原生 tools 参数）
pub fn assemble_stable_system_prompt(files: &[String]) -> String {
    let mut out = String::new();
    for f in files {
        out.push_str(f);
        out.push('\n');
    }
    out
}
