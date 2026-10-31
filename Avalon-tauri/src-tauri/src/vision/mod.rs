// Agent 视觉模块
//
// 职责：图片预处理（本地）、视觉模型观察（远程）、观察缓存、会话附件构造。
// 设计见 doc/v0.7/03-vision-module.md。
//
// 依赖方向：vision 依赖 config / file / llm / usage，不反向依赖 engine。
// engine 只拿 VisionService 把附件变成当前轮消息内容。

mod cache;
mod ocr;
mod prepare;
mod types;
mod understand;

#[allow(unused_imports)] // 供 engine / commands / 后续 view_image 工具按需引用
pub use types::{
    Attachment, ChatAttachment, ImageProfile, Observation, PreparedImage, Thumbnail,
    VisionCapability, MAX_ATTACHMENTS_PER_MESSAGE, OBSERVATION_SCHEMA, OBSERVE_PROMPT_VERSION,
};

use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::config::{ConfigStore, ModelConfig, VisionConfig};
use crate::file::FileStore;
use crate::llm::LlmState;
use crate::usage::UsageStore;

/// 视觉能力的唯一入口。放进 Tauri State，engine / 命令 / 工具共享
pub struct VisionService {
    pub(crate) config: ConfigStore,
    pub(crate) llm: LlmState,
    pub(crate) files: Arc<FileStore>,
    pub(crate) usage: Arc<UsageStore>,
    /// 限制并发视觉调用；固定 8，实际并发由配置里的 concurrency 在调用侧再收一次
    pub(crate) semaphore: Arc<Semaphore>,
}

impl VisionService {
    pub fn new(
        config: ConfigStore,
        llm: LlmState,
        files: Arc<FileStore>,
        usage: Arc<UsageStore>,
    ) -> Self {
        Self {
            config,
            llm,
            files,
            usage,
            semaphore: Arc::new(Semaphore::new(8)),
        }
    }

    /// 当前生效的视觉配置（已归一化到安全范围）
    pub(crate) fn vision_config(&self) -> VisionConfig {
        self.config.get().vision.normalized()
    }

    /// 解析实际使用的视觉模型：
    ///   1. [vision].model 非空 → 按名字查；
    ///   2. 留空 → 活跃模型，但必须标记 supports_vision；
    ///   3. 都不满足 → None（视觉不可用）。
    pub fn resolve_model(&self) -> Option<ModelConfig> {
        let cfg = self.config.get();
        if !cfg.vision.enabled {
            return None;
        }

        let name = cfg.vision.model.trim();
        if !name.is_empty() {
            return cfg.models.iter().find(|m| m.name == name).cloned();
        }

        cfg.active_model_config()
            .filter(|m| m.supports_vision)
            .cloned()
    }

    /// 前端据此决定附件入口是否显示
    pub fn capability(&self) -> VisionCapability {
        let cfg = self.config.get();
        let active_supports_vision = cfg
            .active_model_config()
            .map(|m| m.supports_vision)
            .unwrap_or(false);
        let model = self.resolve_model().map(|m| m.name);

        VisionCapability {
            enabled: cfg.vision.enabled && model.is_some(),
            model,
            active_supports_vision,
            ocr_enabled: cfg.vision.ocr.enabled,
        }
    }
}
