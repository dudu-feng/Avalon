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
    /// 模型列表（连接/鉴权/模型名逐模型独立）
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    /// 当前活跃模型（指向 models[].name）
    #[serde(default)]
    pub active_model: String,
    /// 全局 LLM 行为参数（温度/超时，所有模型共享）
    pub llm: LlmParams,
    /// Embedding 向量化配置
    pub embedding: EmbeddingConfig,
    /// 会话记忆配置
    pub session_memory: SessionMemoryConfig,
    /// Whisper 语音转写配置
    pub whisper: WhisperConfig,
    /// 向量数据库配置
    pub vector: VectorConfig,
    /// 飞书渠道配置（新增段，老配置文件缺失时取默认值）
    #[serde(default)]
    pub feishu: FeishuConfig,
    /// 联网搜索配置（新增段，老配置文件缺失时取默认值）
    #[serde(default)]
    pub search: SearchConfig,

    // —— 运行时派生，不落盘 ——
    /// Avalon-config.toml 完整路径（保存时写回原位置）
    #[serde(skip)]
    pub config_path: PathBuf,
}

impl AppConfig {
    /// 当前活跃模型（按名字查找，找不到返回 None）
    pub fn active_model_config(&self) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.name == self.active_model)
    }
}

/// 路径配置：本项目为「配置驱动」，data 目录等路径是用户可配置项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    /// 共享数据根目录（会话记忆/向量库/模型）。空串 = 按约定推导 project_root/data
    pub data_root: PathBuf,
    /// 文件存放目录（临时文件等）。空串 = 按约定推导 project_root/file
    pub file_root: PathBuf,
}

/// 模型列表项：连接 + 鉴权 + 模型名，逐模型独立
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// 唯一标识（切换锚点，如 "deepseek" / "gpt"）
    pub name: String,
    /// OpenAI 兼容 API 基础 URL
    pub url: String,
    /// API Key（敏感，支持环境变量 AVALON_LLM_API_KEY 覆盖）
    pub key: String,
    /// 实际模型名
    pub modelname: String,
}

/// 全局 LLM 行为参数（所有模型共享）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmParams {
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

/// 向量数据库后端：memory（自研轻量索引）| sqlite（预留扩展）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorBackend {
    Memory,
    Sqlite,
}

/// 向量数据库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorConfig {
    pub backend: VectorBackend,
}

/// 飞书会话隔离粒度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeishuSessionMode {
    /// 每个聊天独立：私聊按人、群聊按群，上下文互不污染
    Isolated,
    /// 所有飞书消息汇入同一个永恒会话（对齐 Python 版），跨群跨私聊记忆连贯
    Unified,
}

/// 飞书渠道配置
///
/// 仅支持「企业自建应用」——长连接（WebSocket）不对商店应用开放。
/// 每个字段都带 `#[serde(default)]`：老配置文件没有 `[feishu]` 段时整段取默认，
/// 用户手写时也允许只写关心的那几项。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeishuConfig {
    /// 是否启用。关闭时应用启动不建立任何连接
    pub enabled: bool,
    /// 应用 App ID（cli_ 开头）
    pub app_id: String,
    /// 应用 App Secret（敏感，支持环境变量 AVALON_FEISHU_APP_SECRET 覆盖）
    pub app_secret: String,
    /// 开放平台域名。飞书 open.feishu.cn / Lark open.larksuite.com
    pub domain: String,
    /// 群聊是否必须 @ 机器人才响应。关掉会让群里每句话都触发一次 ReAct
    pub group_require_mention: bool,
    /// 允许对话的用户 open_id 白名单，空列表 = 不限制
    pub allow_users: Vec<String>,
    /// 主人的 open_id，feishu_notify_owner 的收件人。
    /// 留空时由「第一个私聊机器人且通过准入的用户」自动填充并落盘
    pub owner_open_id: String,
    /// 会话隔离粒度
    pub session_mode: FeishuSessionMode,
    // —— 进度表情。值必须是飞书的 emoji_type 枚举（如 OnIt），不能填 Unicode
    //    字符，否则接口报 231001。任意一项留空即关闭该状态的标记。
    /// 排队等待中（前面还有消息在处理）
    pub queued_reaction: String,
    /// 正在处理
    pub processing_reaction: String,
    /// 处理成功
    pub done_reaction: String,
    /// 处理失败
    pub failed_reaction: String,
    /// 排队积压超限、本条不予处理
    pub rejected_reaction: String,
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            app_id: String::new(),
            app_secret: String::new(),
            domain: "https://open.feishu.cn".to_string(),
            group_require_mention: true,
            allow_users: Vec::new(),
            owner_open_id: String::new(),
            session_mode: FeishuSessionMode::Isolated,
            queued_reaction: "OneSecond".to_string(),
            processing_reaction: "OnIt".to_string(),
            done_reaction: "DONE".to_string(),
            failed_reaction: "ERROR".to_string(),
            rejected_reaction: "Sigh".to_string(),
        }
    }
}

impl FeishuConfig {
    /// 是否具备启动条件：开关打开且凭证齐全
    pub fn is_ready(&self) -> bool {
        self.enabled && !self.app_id.is_empty() && !self.app_secret.is_empty()
    }

    /// 域名去掉尾部斜杠，避免拼出 `https://x//open-apis/...`
    pub fn base_url(&self) -> &str {
        self.domain.trim_end_matches('/')
    }
}

/// 联网搜索（AnySearch）配置。
///
/// 默认关闭：开启意味着模型的查询词会发往第三方服务，这个决定该由用户显式做出，
/// 与飞书渠道同理。api_key 留空也能匿名调用，只是速率受限。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// 是否向模型暴露搜索工具。关闭时 web_search / read_web_page 不出现在工具列表里
    pub enabled: bool,
    /// API Key（敏感，支持环境变量 ANYSEARCH_API_KEY 覆盖）。留空 = 匿名，速率受限
    pub api_key: String,
    /// 服务地址
    pub base_url: String,
    /// 模型未指定条数时的默认值。接口上限为 10
    pub max_results: u32,
    /// 区域偏好 cn / intl，留空交给服务端判断
    pub zone: String,
    /// 单次请求超时
    pub timeout_secs: u64,
    /// 网页正文截断上限（字符）。接口最多返回 5 万字符，整段塞进上下文会挤爆预算
    pub extract_limit: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            base_url: "https://api.anysearch.com".to_string(),
            // 默认 5 而不是接口上限 10：多出来的条目通常只是重复信息，却要多花一倍 token
            max_results: 5,
            zone: String::new(),
            timeout_secs: 30,
            extract_limit: 8000,
        }
    }
}

impl SearchConfig {
    /// 去掉尾部斜杠，避免拼出 `https://x//v1/search`
    pub fn base_url(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }
}
