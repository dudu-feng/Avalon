// 配置数据结构定义
//
// 整体映射 src-tauri/Avalon-config.toml，通过 serde 自动反序列化。
// 字段名与 TOML key 一致（snake_case），枚举用 lowercase 重命名。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 顶层配置：整体映射 Avalon-config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 路径配置（配置驱动：data_root / file_root 用户可指定）
    pub paths: PathsConfig,
    /// LLM 模型配置
    pub llm: LlmConfig,
    /// Embedding 向量化配置
    pub embedding: EmbeddingConfig,
    /// 会话记忆配置
    pub session_memory: SessionMemoryConfig,
    /// Whisper 语音转写配置
    pub whisper: WhisperConfig,

    // —— 运行时派生，不落盘 ——
    /// Avalon-config.toml 完整路径（保存时写回原位置）
    #[serde(skip)]
    pub config_path: PathBuf,
}

/// 路径配置：本项目为「配置驱动」，data 目录等路径是用户可配置项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    /// 共享数据根目录（会话记忆/向量库/模型）。空串 = 按约定推导 project_root/data
    pub data_root: PathBuf,
    /// 文件存放目录（临时文件等）。空串 = 按约定推导 project_root/file
    pub file_root: PathBuf,
}

/// LLM 模型配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// API Key（敏感，支持环境变量 AVALON_LLM_API_KEY 覆盖）
    pub api_key: String,
    /// 模型名称
    pub model: String,
    /// OpenAI 兼容 API 基础 URL
    pub base_url: String,
    /// 对话层温度（原 Python llm.py 硬编码 0.7）
    pub chat_temperature: f32,
    /// JSON/动作层温度（原 Python llm.py 硬编码 0.1）
    pub json_temperature: f32,
    /// 请求超时秒数（原 Python llm.py 硬编码 120）
    pub timeout_secs: u64,
}

/// embedding 模式：用枚举替代魔法字符串
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingMode {
    Local,
    Api,
}

/// embedding 加载时机：lazy（懒加载）| eager（常驻热加载）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingLoadMode {
    Lazy,
    Eager,
}

/// Embedding 向量化配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub mode: EmbeddingMode,
    pub local_model: String,
    pub device: String,
    pub load_mode: EmbeddingLoadMode,
    pub api_key: String,
    pub api_model: String,
    pub api_base_url: String,
}

/// 会话记忆检索模式：对应 search_session_memory 工具的三种模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Semantic,
    Keyword,
    Hybrid,
}

/// 会话记忆配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryConfig {
    /// 自动压缩阈值（输入 token）
    pub compress_threshold: usize,
    /// 压缩块超此数量触发渐进式总结
    pub max_chunks: usize,
    /// 系统提示加载最近 N 个压缩块
    pub context_chunks: usize,
    /// 检索模式
    pub search_mode: SearchMode,
}

/// Whisper 语音转写配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    pub model_name: String,
    pub device: String,
}
