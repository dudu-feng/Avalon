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
    /// QQ 渠道配置（新增段，老配置文件缺失时取默认值）
    #[serde(default)]
    pub qq: QqConfig,
    /// 联网搜索配置（新增段，老配置文件缺失时取默认值）
    #[serde(default)]
    pub search: SearchConfig,
    /// Agent 基础工具的沙箱配置（新增段，老配置文件缺失时取默认值）
    #[serde(default)]
    pub tools: ToolsConfig,
    /// 视觉输入配置（新增段，老配置文件缺失时取默认值）
    #[serde(default)]
    pub vision: VisionConfig,

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
    /// 文件存放目录（普通文件/临时文件）。空串 = data_root/file；图片在其 image 子目录
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
    /// 是否支持图片输入。缺省 false —— 不能把纯文本模型误当视觉模型，
    /// 否则发图时提供商会直接 400
    #[serde(default)]
    pub supports_vision: bool,
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
    /// 旧版配置里的主人 open_id。主人已迁入通讯录（`owners.feishu`），
    /// 这里只在启动时读取一次做迁移，运行期不再使用；序列化时跳过，
    /// 避免把已经废弃的字段重新写回配置文件
    #[serde(default, skip_serializing)]
    pub owner_open_id: String,
    /// 会话隔离粒度
    pub session_mode: FeishuSessionMode,
    /// 收到图片消息后、等用户说明意图前发送的询问文本。
    /// 外部渠道的图片与文字通常分两条消息到达，先确认再挂起，避免在用户
    /// 还没提出问题时提前调用视觉模型。留空 = 不询问，图片立即交给模型处理
    pub image_ack_text: String,
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
            image_ack_text:
                "已收到图片，请问需要我做什么？可以直接回复你的问题，我会结合这张图回答。"
                    .to_string(),
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

/// QQ 渠道的后端。OneBot 11 只预留配置结构，默认不实现
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QqBackend {
    /// QQ 开放平台官方机器人（唯一的正式实现）
    Official,
    /// OneBot 11 协议端（NapCat 等）。非官方、封号风险，代码未实现
    Onebot,
}

/// QQ 会话隔离粒度，语义与飞书一致
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QqSessionMode {
    /// 每个群 / 私聊独立上下文
    Isolated,
    /// 全部 QQ 消息汇入同一个会话
    Unified,
}

/// QQ 官方机器人配置，对应 [qq.official]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QqOfficialConfig {
    /// 开放平台 AppID
    pub app_id: String,
    /// AppSecret（敏感，支持环境变量 AVALON_QQ_APP_SECRET 覆盖）
    pub app_secret: String,
    /// 沙箱环境。true = sandbox.api.sgroup.qq.com
    pub sandbox: bool,
    /// 订阅的 intents。群聊 + 单聊消息 = GROUP_AND_C2C_EVENT = 1 << 25
    pub intents: i64,
    /// C2C 是否尝试原生流式；群聊始终非流式。false 仅用于排障
    pub stream_c2c: bool,
    /// HTTP 请求超时秒数
    pub timeout_secs: u64,
}

impl Default for QqOfficialConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_secret: String::new(),
            sandbox: true,
            intents: 1 << 25,
            stream_c2c: true,
            timeout_secs: 30,
        }
    }
}

impl QqOfficialConfig {
    /// 正式环境域名。官方 2026-08 起统一到 api.bot.qq.com；
    /// 旧域名 api.sgroup.qq.com 仍被大量客户端使用，这里按最新公告取新域名
    pub fn api_base(&self) -> &'static str {
        if self.sandbox {
            "https://sandbox.api.sgroup.qq.com"
        } else {
            "https://api.bot.qq.com"
        }
    }

    /// 取 token 的端点与 API 域名不同，沙箱/正式共用
    pub fn token_url(&self) -> &'static str {
        "https://bots.qq.com/app/getAppAccessToken"
    }

    /// intents 为 0 时退回群聊 + 单聊事件，避免写错一个数字后什么都不收
    pub fn effective_intents(&self) -> i64 {
        if self.intents <= 0 {
            1 << 25
        } else {
            self.intents
        }
    }
}

/// OneBot 11 配置，对应 [qq.onebot]。
/// 仅预留结构：不实现代码、设置页标注未支持，避免把非官方路径做成默认能力
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QqOnebotConfig {
    /// NapCat 等协议端的正向 WebSocket 地址
    pub ws_url: String,
    /// 协议端 access_token（敏感，支持 AVALON_QQ_ONEBOT_TOKEN 覆盖）
    pub access_token: String,
    /// 机器人 QQ 号，用于判断 at 段
    pub self_id: i64,
}

/// QQ 渠道配置，对应 [qq]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QqConfig {
    /// 是否启用。关闭时应用启动不建立任何连接
    pub enabled: bool,
    /// 后端。official 是唯一实现的路径
    pub backend: QqBackend,
    /// 会话隔离粒度
    pub session_mode: QqSessionMode,
    /// 群聊是否只响应 @ 机器人的消息。全量群消息模式下尤其重要
    pub group_require_mention: bool,
    /// 允许对话的用户 openid 白名单，空列表 = 不限制
    pub allow_users: Vec<String>,
    /// 旧版配置里的主人 openid。主人已迁入通讯录（`owners.qq`），
    /// 仅用于启动迁移；序列化时跳过
    #[serde(default, skip_serializing)]
    pub owner_open_id: String,
    /// 收到纯图片消息后、等用户说明意图前发送的询问文本。
    /// 留空 = 不询问，图片立即交给模型处理
    pub image_ack_text: String,
    pub official: QqOfficialConfig,
    pub onebot: QqOnebotConfig,
}

impl Default for QqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: QqBackend::Official,
            session_mode: QqSessionMode::Isolated,
            group_require_mention: true,
            allow_users: Vec::new(),
            owner_open_id: String::new(),
            image_ack_text:
                "已收到图片，请问需要我做什么？可以直接回复你的问题，我会结合这张图回答。"
                    .to_string(),
            official: QqOfficialConfig::default(),
            onebot: QqOnebotConfig::default(),
        }
    }
}

impl QqConfig {
    /// 是否具备启动条件：开关打开、选的是官方后端且凭证齐全
    pub fn is_ready(&self) -> bool {
        self.enabled
            && self.backend == QqBackend::Official
            && !self.official.app_id.is_empty()
            && !self.official.app_secret.is_empty()
    }
}

/// Agent 基础工具（文件 / 终端）的沙箱配置。
///
/// 两条边界对所有来源统一生效 —— 不按渠道分档。桌面端的模型同样会被
/// read_web_page 抓回的内容注入，按来源分档只是把高危操作挪个地方。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    /// 文件工具允许访问的根目录（读写同边界）。
    ///
    /// 用 Option 而不是空 Vec 是为了区分两种意图：
    ///   字段缺失 → 取默认 data_root/workspace
    ///   显式写 [] → 禁止全部文件操作
    ///
    /// skip_serializing_if 不能省：没配过的用户存一次设置就会被写进一行
    /// `workspace_roots = ...`，把「按默认推导」固化成一个绝对路径，
    /// 换台机器或改了 data_root 就废
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_roots: Option<Vec<String>>,
    /// 终端可执行的命令白名单（裸命令名，不含路径与扩展名）。留空 = 禁用终端。
    ///
    /// 注意这个列表的性质：一旦放进解释器（python / node / powershell）
    /// 或带插件机制的工具（git 的 -c core.pager、npm 的 run 脚本），
    /// 限制就从「防恶意」退化成「防误操作」—— 一行代码即可绕开全部约束。
    pub shell_allowlist: Vec<String>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            workspace_roots: None,
            // 只放只读诊断命令，且都是真实可执行文件。
            // 刻意不含 findstr（/f: 能读任意文件，一条命令废掉整个读边界，
            // 且功能已被 read_file 覆盖）与 git（-c core.pager 是任意命令执行）。
            // dir / type 不在列表里不是遗漏 —— 它们是 cmd 内建，没有对应 exe
            shell_allowlist: if cfg!(windows) {
                [
                    "where",
                    "ping",
                    "ipconfig",
                    "tasklist",
                    "systeminfo",
                    "hostname",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect()
            } else {
                ["ls", "ping", "ps", "uname", "hostname", "df"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            },
        }
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

/// 图像观察策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObserveMode {
    /// 首次进入会话即生成观察并缓存（历史/压缩/非视觉模型都能用）
    Always,
    /// 当前轮直接发图，只有需要文本表示时才生成
    Lazy,
}

/// 本地 OCR 配置。引擎首次调用时才加载，启动不占内存
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OcrConfig {
    pub enabled: bool,
    /// none | onnx
    pub engine: String,
    /// 低于该置信度时在工具输出里标注「低置信度」；不决定是否调用 VLM
    pub min_confidence: f32,
    /// 模型目录；留空 = data_root/models/ocr（det.onnx / rec.onnx / cls.onnx / ppocr_keys_v1.txt）
    pub model_dir: String,
    /// 是否用方向分类器把倒置（180°）的文本行转正后再识别。
    /// 需要同目录下 cls.onnx；模型缺失时自动降级，不影响其余识别
    pub use_angle_cls: bool,
    /// 判定「倒置」的概率阈值：cls 输出 180° 的概率达到该值才旋转
    pub cls_threshold: f32,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: "none".to_string(),
            min_confidence: 0.75,
            model_dir: String::new(),
            use_angle_cls: true,
            cls_threshold: 0.9,
        }
    }
}

/// 视觉输入配置（对应 [vision] 段）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VisionConfig {
    /// 是否启用图片输入。关闭时附件入口隐藏、视觉工具不注册
    pub enabled: bool,
    /// 视觉模型：指向 models[].name；留空 = 活跃模型（前提 supports_vision）
    pub model: String,
    pub observe_mode: ObserveMode,
    /// 送模型前缩放到的最长边
    pub max_edge: u32,
    /// 编码后字节上限
    pub max_bytes: u64,
    pub jpeg_quality: u8,
    /// OpenAI 兼容的 image_url.detail：auto / low / high；留空则不传
    pub detail: String,
    pub timeout_secs: u64,
    pub concurrency: usize,
    /// VLM/OCR 缓存保留天数；0 = 不自动清理
    pub cache_ttl_days: i64,
    /// 首次向某个视觉模型发送图片前是否弹确认。
    /// 属于前端策略，但随配置持久化，换机器/重装后仍然生效
    pub confirm_before_send: bool,
    pub ocr: OcrConfig,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: String::new(),
            observe_mode: ObserveMode::Always,
            max_edge: 1568,
            max_bytes: 4 * 1024 * 1024,
            jpeg_quality: 85,
            detail: "auto".to_string(),
            timeout_secs: 60,
            concurrency: 2,
            cache_ttl_days: 30,
            confirm_before_send: true,
            ocr: OcrConfig::default(),
        }
    }
}

impl VisionConfig {
    /// 归一化到可执行范围，避免配置里写 0 或离谱值把流水线弄坏
    pub fn normalized(&self) -> Self {
        let mut cfg = self.clone();
        cfg.max_edge = cfg.max_edge.clamp(256, 8192);
        cfg.max_bytes = cfg.max_bytes.clamp(64 * 1024, 32 * 1024 * 1024);
        cfg.jpeg_quality = cfg.jpeg_quality.clamp(40, 95);
        cfg.timeout_secs = cfg.timeout_secs.max(5);
        cfg.concurrency = cfg.concurrency.clamp(1, 8);
        cfg.ocr.cls_threshold = cfg.ocr.cls_threshold.clamp(0.5, 0.99);
        cfg
    }
}
