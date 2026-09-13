// Prompt 提示词组装模块
//
// 职责：对话层 system prompt 的组装 —— 从灵魂注册表（SoulRegistry）读条目
// + 会话上下文。工具走 OpenAI 原生 tools 参数传入，不再写进 system prompt。
// 依赖反转：prompt 依赖 soul 的 SoulRegistry（Arc 共享），tool/session 以 &str 参数注入。
// action/compress 提示词模板仍硬编码在 llm/client.rs 与 templates.rs（协议耦合）。

#![allow(dead_code)] // prompt 模块供未来 engine 引用，当前无调用方，接入后移除

pub mod templates;

use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::soul::SoulRegistry;

pub use templates::build_compress_prompt;

/// 提示词组装器：从灵魂注册表读条目拼 system prompt，带线程安全缓存
pub struct PromptAssembler {
    registry: Arc<SoulRegistry>,
    /// None = 尚未加载；Some = 已缓存的注入片段列表
    cache: RwLock<Option<Vec<String>>>,
}

impl PromptAssembler {
    /// 从灵魂注册表构造
    pub fn new(registry: Arc<SoulRegistry>) -> Self {
        Self {
            registry,
            cache: RwLock::new(None),
        }
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

    /// 清空缓存（条目变更后调用，下次 load_files 惰性重载）
    pub fn refresh(&self) {
        *self.cache.write().unwrap() = None;
    }

    /// 对话层完整组装：load_files + 纯函数拼接
    pub fn assemble_chat_prompt(&self, session_context: &str) -> Result<String> {
        let files = self.load_files()?;
        Ok(assemble_chat_system_prompt(&files, session_context))
    }
}

/// 纯函数：文件列表 + 会话上下文 → 完整 system prompt（工具走原生 tools 参数，不再写进提示词）
pub fn assemble_chat_system_prompt(files: &[String], session_context: &str) -> String {
    let mut out = String::new();
    for f in files {
        out.push_str(f);
        out.push('\n');
    }
    if !session_context.trim().is_empty() {
        out.push_str("\n=====历史会话记录=====\n");
        out.push_str(session_context);
    }
    out
}
