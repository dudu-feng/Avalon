// 文件存储的数据结构
//
// 落盘布局与命名规则见 doc/v0.7/02-file-storage.md。

use serde::{Deserialize, Serialize};

/// sidecar schema 版本。字段有不兼容变更时递增，读取方据此决定兼容策略
pub const SIDECAR_SCHEMA: u32 = 1;

/// 文件大类：由最终 MIME 推导，决定是否进 image/ 目录
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Image,
    Document,
    Audio,
    Video,
    Archive,
    Other,
}

impl FileKind {
    /// 从 MIME 判断大类。拿不到 MIME 时归入 Other，不做猜测
    pub fn from_mime(mime: &str) -> Self {
        if mime.starts_with("image/") {
            Self::Image
        } else if mime.starts_with("audio/") {
            Self::Audio
        } else if mime.starts_with("video/") {
            Self::Video
        } else if is_archive_mime(mime) {
            Self::Archive
        } else if is_document_mime(mime) {
            Self::Document
        } else {
            Self::Other
        }
    }

    pub fn is_image(self) -> bool {
        self == Self::Image
    }
}

fn is_archive_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip"
            | "application/x-zip-compressed"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-7z-compressed"
            | "application/x-rar-compressed"
            | "application/vnd.rar"
            | "application/x-bzip2"
            | "application/zstd"
    )
}

fn is_document_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/pdf"
                | "application/json"
                | "application/xml"
                | "application/rtf"
                | "application/msword"
                | "application/vnd.ms-excel"
                | "application/vnd.ms-powerpoint"
        )
        // Office Open XML（docx / xlsx / pptx）与 OpenDocument
        || mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.oasis.opendocument.")
}

/// 文件来源：决定文件名前缀，便于不打开 sidecar 也能分辨出处
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileSource {
    /// 前端用户上传（聊天附件、拖拽、粘贴）
    Upload,
    /// 飞书渠道下载
    Feishu,
    /// Agent 生成（绘图、表情包、桌宠素材）
    Generated,
    /// Agent 从网络下载
    Web,
    /// 剪贴板
    Clipboard,
    /// 由已有文件派生（裁剪、缩放、滤镜）
    Edit,
}

impl FileSource {
    /// 文件名前缀，固定两三个字符，保持可读
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Upload => "up",
            Self::Feishu => "fs",
            Self::Generated => "gen",
            Self::Web => "web",
            Self::Clipboard => "clip",
            Self::Edit => "edit",
        }
    }

    /// 宽松解析：同时接受完整名字与文件名前缀。
    /// 前端传的是完整名字；日志/自动化里偶尔会用前缀，一并认掉
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "upload" | "up" => Some(Self::Upload),
            "feishu" | "fs" => Some(Self::Feishu),
            "generated" | "gen" => Some(Self::Generated),
            "web" => Some(Self::Web),
            "clipboard" | "clip" => Some(Self::Clipboard),
            "edit" => Some(Self::Edit),
            _ => None,
        }
    }
}

/// 落盘结果。给前端的引用只包含相对路径，不暴露绝对路径
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFile {
    /// 文件名主体（不含扩展名），也是 sidecar 的前缀
    pub id: String,
    /// 相对 file_root 的路径，统一用 / 分隔
    pub path: String,
    pub kind: FileKind,
    pub mime: String,
    pub size: u64,
    /// 内容 SHA-256，小写十六进制
    pub sha256: String,
    /// 本地时间，RFC 3339
    pub created_at: String,
    pub original_name: Option<String>,
    /// sidecar 相对路径；未写时为 None
    pub meta_path: Option<String>,
}

/// 与主文件同目录的 `<id>.meta.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidecar {
    pub schema: u32,
    pub id: String,
    pub kind: FileKind,
    pub source: FileSource,
    pub original_name: Option<String>,
    pub mime: String,
    pub ext: String,
    pub size: u64,
    pub sha256: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// 图片尺寸与感知哈希，由后续预处理流水线填充
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageMeta>,
    /// 缓存产物指针，路径相对 file_root；缓存被清后置空即可重建
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived: Option<DerivedPaths>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phash: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DerivedPaths {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vlm: Option<String>,
}

/// 一次清理的结果，供命令返回值与日志使用
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupReport {
    pub removed_files: u64,
    pub removed_dirs: u64,
    pub freed_bytes: u64,
}

/// 可重建产物的缓存分类，对应 `file/cache/<name>/`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    /// 预览缩略图
    Thumb,
    /// 本地 OCR 结果
    Ocr,
    /// VLM 观察结果
    Vlm,
}

impl CacheKind {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Thumb => "thumb",
            Self::Ocr => "ocr",
            Self::Vlm => "vlm",
        }
    }
}
