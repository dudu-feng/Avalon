// Prompt 提示词组装模块
//
// 职责：对话层 system prompt 的组装 —— 加载用户自定义 .md 提示词文件（灵魂/画像）
// + 内置标记协议约束 + 工具列表 + 会话上下文。
// 只依赖 config（拿 prompt 目录路径）；tool/session 以 &str 参数注入（依赖反转）。
// action/compress 提示词模板仍硬编码在 llm/client.rs（协议耦合，本期不迁移）。

#![allow(dead_code)] // prompt 模块供未来 engine 引用，当前无调用方，接入后移除

pub mod templates;

use std::path::PathBuf;
use std::sync::RwLock;

use anyhow::{Context, Result};

use crate::config::AppConfig;

pub use templates::{build_action_prompt, build_compress_prompt, BASIC_SETTING, RESPONSE_TEMPLATE};

/// 提示词组装器：从 prompt 目录加载 .md 文件，带线程安全缓存
pub struct PromptAssembler {
    dir: PathBuf,
    /// None = 尚未加载；Some = 已缓存「基本设定 + 各 .md 内容」列表
    cache: RwLock<Option<Vec<String>>>,
}

impl PromptAssembler {
    /// 从配置构造（dir = config.prompt_file_path()，即 data_root/memory/prompt）
    pub fn new(config: &AppConfig) -> Self {
        Self::from_dir(config.prompt_file_path())
    }

    /// 从指定目录构造（测试注入临时目录）
    pub fn from_dir(dir: PathBuf) -> Self {
        Self {
            dir,
            cache: RwLock::new(None),
        }
    }

    /// 加载提示词文件：基本设定置首 + 目录下 *.md 按文件名排序，带缓存。
    /// 目录不存在 / 无 .md 时不报错（基本设定兜底），仅打警告。
    pub fn load_files(&self) -> Result<Vec<String>> {
        if let Some(cached) = self.cache.read().unwrap().as_ref() {
            return Ok(cached.clone());
        }

        let mut files = vec![BASIC_SETTING.to_string()];
        if !self.dir.exists() {
            eprintln!("[Prompt] 提示词目录不存在: {}", self.dir.display());
        } else {
            let mut names: Vec<PathBuf> = std::fs::read_dir(&self.dir)
                .with_context(|| format!("读取提示词目录失败: {}", self.dir.display()))?
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    (path.extension().and_then(|e| e.to_str()) == Some("md")).then_some(path)
                })
                .collect();
            names.sort(); // 确定性顺序（Python os.listdir 无序）
            for path in names {
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("读取提示词文件失败: {}", path.display()))?;
                let content = content.trim();
                if !content.is_empty() {
                    files.push(content.to_string());
                }
            }
        }

        *self.cache.write().unwrap() = Some(files.clone());
        Ok(files)
    }

    /// 清空缓存（修改 .md 后调用，下次 load_files 惰性重载）
    pub fn refresh(&self) {
        *self.cache.write().unwrap() = None;
    }

    /// 对话层完整组装：load_files + 纯函数拼接
    pub fn assemble_chat_prompt(&self, tool_list: &str, session_context: &str) -> Result<String> {
        let files = self.load_files()?;
        Ok(assemble_chat_system_prompt(&files, tool_list, session_context))
    }
}

/// 纯函数：文件列表 + 标记协议约束 + 工具列表 + 会话上下文 → 完整 system prompt
pub fn assemble_chat_system_prompt(
    files: &[String],
    tool_list: &str,
    session_context: &str,
) -> String {
    let mut out = String::new();
    for f in files {
        out.push_str(f);
        out.push('\n');
    }
    out.push_str(RESPONSE_TEMPLATE);
    out.push_str("\n\n");
    if !tool_list.trim().is_empty() {
        out.push_str(tool_list);
        out.push('\n');
    }
    if !session_context.trim().is_empty() {
        out.push_str("\n=====历史会话记录=====\n");
        out.push_str(session_context);
    }
    out
}
