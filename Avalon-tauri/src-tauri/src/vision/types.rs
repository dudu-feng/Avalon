// 视觉模块的数据结构
//
// 设计见 doc/v0.7/03-vision-module.md。核心约定：
//   - 会话里只存图片相对路径与观察文本，不存 base64；
//   - 观察结果按「内容 sha + 预处理参数 + 模型 + 提示词版本」缓存；
//   - 图片字节只在当前轮请求里出现。

use serde::{Deserialize, Serialize};

/// 前端传来的附件引用：只信任路径，其余信息由后端从 sidecar 补齐
#[derive(Debug, Clone, Deserialize)]
pub struct ChatAttachment {
    pub path: String,
}

/// 落在 `Message::User.meta.attachments` 里的附件快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub path: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
    /// 已生成的图像观察；后续轮次靠它回忆图片内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_text: Option<String>,
    /// VLM 给出的短标签；限制数量与长度，避免会话元数据膨胀
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// 生成这条观察的模型（modelname），用于追溯观察来源
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_model: Option<String>,
    /// 观察生成时间（RFC3339）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
}

/// 原图的基础画像。字段只描述「是什么」，不做语义判断
#[derive(Debug, Clone)]
pub struct ImageProfile {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub has_alpha: bool,
    pub animated: bool,
    pub frames: u32,
    pub source_bytes: u64,
}

/// 预处理后的缩略图
#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub ext: &'static str,
}

/// 送进视觉模型的图片字节
#[derive(Debug, Clone)]
pub struct PreparedImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
    /// 原图内容哈希，用于缓存键
    pub sha256: String,
    pub profile: ImageProfile,
    pub thumbnail: Thumbnail,
}

/// 视觉模型对一张图的观察结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub schema: u32,
    pub sha256: String,
    pub model: String,
    pub prompt_version: u32,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_text: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub elapsed_ms: u64,
    pub created_at: String,
    /// 运行时标记：本次是缓存命中还是真实调用；不写入缓存文件
    #[serde(skip)]
    pub cached: bool,
}

/// 前端据此决定附件入口是否显示
#[derive(Debug, Clone, Serialize)]
pub struct VisionCapability {
    pub enabled: bool,
    /// 实际使用的视觉模型名（`models[].name`）
    pub model: Option<String>,
    /// 活跃模型本身是否能看图
    pub active_supports_vision: bool,
    pub ocr_enabled: bool,
}

/// 视觉缓存 schema 版本
pub const OBSERVATION_SCHEMA: u32 = 1;

/// 观察提示词版本。修改提示词时递增，旧缓存自动失效
pub const OBSERVE_PROMPT_VERSION: u32 = 1;

/// 单条消息最多处理的图片数；超出部分只保留文字提示
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 4;

/// 解码前的像素上限，防解压炸弹
pub const MAX_PIXELS: u64 = 40_000_000;

/// 解码前的单边上限
pub const MAX_IMAGE_SIDE: u32 = 16_384;

/// 观察文本写入会话时的截断长度
pub const META_OBSERVATION_LIMIT: usize = 1000;

/// OCR 文本写入会话时的截断长度
pub const META_OCR_LIMIT: usize = 4000;

/// 标签写入会话时的最大数量
pub const META_LABELS_MAX: usize = 8;

/// 单个标签写入会话时的截断长度
pub const META_LABEL_LIMIT: usize = 32;
