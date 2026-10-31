// 本地 OCR：引擎 trait + 服务方法 + 结果缓存
//
// OCR 是模型可调用的工具，不是自动前置步骤：它只提取文字，不做语义理解。
// 引擎未实现或模型缺失时 build_engine 返回 None，ocr_image 工具不会注册。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::config::OcrConfig;
use crate::file::{CacheKind, ImageMeta, Sidecar};

use super::cache;
use super::ocr_onnx::{self, OnnxOcr};
use super::types::PreparedImage;
use super::VisionService;

/// OCR 缓存 schema。修改结果结构时递增
pub const OCR_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub schema: u32,
    /// 产出结果的引擎名（缓存键的一部分）
    pub engine: String,
    pub text: String,
    pub lines: Vec<OcrLine>,
    pub confidence: f32,
    pub elapsed_ms: u64,
    pub created_at: String,
    /// 运行时标记：本次是缓存命中还是真实调用；不写入缓存文件
    #[serde(skip)]
    pub cached: bool,
}

impl OcrResult {
    /// 引擎实现用这个构造；schema/engine/时间戳由服务层统一覆盖
    pub fn new(text: String, lines: Vec<OcrLine>, confidence: f32) -> Self {
        Self {
            schema: OCR_SCHEMA,
            engine: String::new(),
            text,
            lines,
            confidence,
            elapsed_ms: 0,
            created_at: String::new(),
            cached: false,
        }
    }
}

pub trait OcrEngine: Send + Sync {
    /// 引擎标识，参与缓存键；同一引擎升级模型时应变更版本号
    fn name(&self) -> &'static str;

    /// 同步识别。调用方在 spawn_blocking 里执行，避免阻塞对话流
    fn recognize(&self, image: &PreparedImage) -> Result<OcrResult>;
}

/// 未启用 OCR 时的空实现：永远返回「没有结果」
#[allow(dead_code)]
pub struct NoopOcr;

impl OcrEngine for NoopOcr {
    fn name(&self) -> &'static str {
        "none"
    }

    fn recognize(&self, _image: &PreparedImage) -> Result<OcrResult> {
        bail!("OCR 未启用")
    }
}

/// OCR 引擎句柄：持有配置与模型目录，真正加载推迟到首次识别。
///
/// 用 `Mutex<Option<_>>` 而不是 `OnceLock`：需要支持卸载（关闭 OCR /
/// 切换模型后释放 30–80 MB 的会话内存），且加载失败不能缓存 —— 用户
/// 放好模型后下一次调用应当能重试成功，而不是要求重启进程。
pub struct OcrHandle {
    model_dir: PathBuf,
    engine: Mutex<Option<Arc<dyn OcrEngine>>>,
    /// 串行化首次加载，避免并发请求各解析一份模型
    loading: AsyncMutex<()>,
}

impl OcrHandle {
    pub fn new(model_dir: PathBuf) -> Self {
        Self {
            model_dir,
            engine: Mutex::new(None),
            loading: AsyncMutex::new(()),
        }
    }

    /// 模型资产是否齐全。只 stat 文件，不加载模型 —— 工具注册时调用
    pub fn models_present(&self) -> bool {
        ocr_onnx::models_present(&self.model_dir)
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    /// 已加载的引擎；不触发加载
    pub fn loaded(&self) -> Option<Arc<dyn OcrEngine>> {
        self.engine.lock().ok().and_then(|guard| guard.clone())
    }

    /// 懒加载：已加载秒回；未加载时在 spawn_blocking 里解析模型
    pub async fn get(&self) -> Result<Arc<dyn OcrEngine>> {
        if let Some(engine) = self.loaded() {
            return Ok(engine);
        }

        let _guard = self.loading.lock().await;
        // 双重检查：等锁期间可能已有其他调用完成加载
        if let Some(engine) = self.loaded() {
            return Ok(engine);
        }

        let dir = self.model_dir.clone();
        let engine = tokio::task::spawn_blocking(move || -> Result<Arc<dyn OcrEngine>> {
            Ok(Arc::new(OnnxOcr::load(&dir)?))
        })
        .await
        .map_err(|e| anyhow::anyhow!("OCR 加载任务失败: {e}"))??;

        if let Ok(mut guard) = self.engine.lock() {
            *guard = Some(engine.clone());
        }
        Ok(engine)
    }

    /// 释放引擎。关闭 OCR、换引擎或换模型目录后调用
    pub fn unload(&self) {
        if let Ok(mut guard) = self.engine.lock() {
            *guard = None;
        }
    }
}

/// 根据配置构建引擎句柄。不加载模型，只记录目录与开关。
///
/// 返回 None 表示「OCR 不可用」，工具层据此不注册 ocr_image。
pub fn build_engine(cfg: &OcrConfig, model_dir: PathBuf) -> Option<Arc<OcrHandle>> {
    if !cfg.enabled {
        return None;
    }

    match cfg.engine.trim().to_ascii_lowercase().as_str() {
        "none" | "" => None,
        "onnx" => {
            let handle = OcrHandle::new(model_dir);
            if handle.models_present() {
                // 这里只报告状态，不加载 —— 首次 ocr_image 才解析模型
                log::info!(
                    target: "vision",
                    "OCR 引擎已配置（懒加载），模型目录: {}",
                    handle.model_dir().display()
                );
            } else {
                log::warn!(
                    target: "vision",
                    "OCR 模型缺失，ocr_image 不会注册；请放置 det.onnx / rec.onnx / {} 到 {}",
                    ocr_onnx::DICT_FILE,
                    handle.model_dir().display()
                );
            }
            Some(Arc::new(handle))
        }
        "tract" => {
            log::warn!(target: "vision", "OCR engine=tract 尚未实现，ocr_image 不会注册");
            None
        }
        other => {
            log::warn!(
                target: "vision",
                "未知的 OCR engine={other}，ocr_image 不会注册"
            );
            None
        }
    }
}

impl VisionService {
    /// OCR 工具是否可注册：视觉总开关 + OCR 开关 + 引擎配置有效 + 模型文件齐全。
    /// 注意这里不加载模型，注册与否不影响启动内存
    pub fn ocr_available(&self) -> bool {
        let cfg = self.config.get();
        cfg.vision.enabled
            && cfg.vision.ocr.enabled
            && self.ocr.as_ref().is_some_and(|h| h.models_present())
    }

    /// 卸载已加载的 OCR 引擎（设置变更时调用）
    pub fn unload_ocr(&self) {
        if let Some(handle) = &self.ocr {
            handle.unload();
        }
    }

    /// 识别 file_root 下的图片（写入 sidecar.derived.ocr）
    pub async fn ocr_relative(&self, relative: &str) -> Result<OcrResult> {
        let prepared = self.prepare(relative).await?;
        let sidecar = match self.files.read_sidecar(relative) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(target: "vision", "读取 OCR sidecar 失败: {e:#}");
                None
            }
        };
        self.ocr_prepared(Some(relative), sidecar.as_ref(), &prepared)
            .await
    }

    /// 识别工作区内任意图片（不写 sidecar；缓存仍按内容 sha + 引擎命中）。
    ///
    /// 路径边界由调用方（ocr_image 工具）负责校验，这里只做解码与识别。
    pub async fn ocr_absolute(&self, path: &Path) -> Result<OcrResult> {
        let cfg = self.vision_config();
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("读取图片失败: {}", path.display()))?;
        let prepared = {
            // 许可只覆盖解码/缩放；识别阶段由 ocr_prepared 自己再领一次
            let _permit = self
                .semaphore
                .acquire_many(cfg.concurrency as u32)
                .await
                .context("获取图片预处理许可失败")?;
            tokio::task::spawn_blocking(move || super::prepare::prepare(&bytes, &cfg))
                .await
                .map_err(|e| anyhow::anyhow!("预处理任务失败: {e}"))??
        };
        self.ocr_prepared(None, None, &prepared).await
    }

    /// 用已预处理的结果识别；命中缓存时不再调用引擎
    async fn ocr_prepared(
        &self,
        relative: Option<&str>,
        sidecar: Option<&Sidecar>,
        prepared: &PreparedImage,
    ) -> Result<OcrResult> {
        let handle = self
            .ocr
            .as_ref()
            .context("OCR 未启用（[vision.ocr].enabled / engine）")?;
        let engine = handle.get().await?;
        let cfg = self.vision_config();
        let engine_name = engine.name();
        let cache_name = ocr_cache_name(&prepared.sha256, engine_name);
        let cache_path = self.files.cache_dir(CacheKind::Ocr).join(&cache_name);

        if let Some(mut cached) = cache::read_json::<OcrResult>(&cache_path) {
            if cached.schema == OCR_SCHEMA && cached.engine == engine_name {
                cached.cached = true;
                return Ok(cached);
            }
        }

        let _permit = self
            .semaphore
            .acquire_many(cfg.concurrency as u32)
            .await
            .context("获取 OCR 并发许可失败")?;

        let started = Instant::now();
        let image = prepared.clone();
        let mut result = tokio::task::spawn_blocking(move || engine.recognize(&image))
            .await
            .map_err(|e| anyhow::anyhow!("OCR 任务失败: {e}"))??;

        // 引擎只负责 text/lines/confidence，元信息由服务层统一补齐
        result.schema = OCR_SCHEMA;
        result.engine = engine_name.to_string();
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        result.created_at = Local::now().to_rfc3339();
        result.cached = false;

        if let Err(e) = cache::write_json(&cache_path, &result) {
            log::warn!(target: "vision", "写 OCR 缓存失败: {e:#}");
        }

        if let (Some(relative), Some(_)) = (relative, sidecar) {
            let cache_rel = self.files.cache_relative(CacheKind::Ocr, &cache_name);
            let width = prepared.profile.width;
            let height = prepared.profile.height;
            let phash = prepared.phash.clone();
            if let Err(e) = self.files.update_sidecar(relative, |s| {
                s.image = Some(ImageMeta {
                    width,
                    height,
                    phash: Some(phash),
                });
                s.derived.get_or_insert_with(Default::default).ocr = Some(cache_rel);
            }) {
                log::warn!(target: "vision", "更新 OCR sidecar 失败: {e:#}");
            }
        }

        Ok(result)
    }
}

/// OCR 缓存文件名：`<sha256>-<engine>.json`
pub fn ocr_cache_name(sha256: &str, engine: &str) -> String {
    format!("{sha256}-{}.json", sanitize_token(engine))
}

/// 引擎名来自代码，但仍限制字符集，避免配置/插件传入奇怪字符
fn sanitize_token(text: &str) -> String {
    let out: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "engine".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_name_contains_sha_and_engine() {
        let name = ocr_cache_name("abc123", "ppocr-v4");
        assert_eq!(name, "abc123-ppocr-v4.json");
    }

    #[test]
    fn cache_name_sanitizes_engine() {
        let name = ocr_cache_name("abc", "../evil engine");
        assert_eq!(name, "abc-___evil_engine.json");
        assert!(!name.contains('/'));
    }

    #[test]
    fn result_constructor_sets_defaults() {
        let result = OcrResult::new("hello".into(), vec![], 0.9);
        assert_eq!(result.schema, OCR_SCHEMA);
        assert!(result.engine.is_empty());
        assert!(!result.cached);
    }

    #[test]
    fn build_engine_respects_switch_and_engine() {
        let mut cfg = OcrConfig::default();
        let dir = PathBuf::from("nonexistent-ocr-models");

        // 总开关关闭 → 不创建句柄
        assert!(build_engine(&cfg, dir.clone()).is_none());

        // 开启但 engine = none → 不创建句柄
        cfg.enabled = true;
        assert!(build_engine(&cfg, dir.clone()).is_none());

        // onnx → 即使模型缺失也返回句柄，由 ocr_available 决定是否注册工具
        cfg.engine = "onnx".to_string();
        let handle = build_engine(&cfg, dir.clone()).expect("onnx 应返回句柄");
        assert!(!handle.models_present());
        assert!(handle.loaded().is_none(), "构造句柄不应加载模型");
    }

    #[tokio::test]
    async fn ocr_handle_load_failure_is_not_cached() {
        let handle = OcrHandle::new(PathBuf::from("nonexistent-ocr-models"));
        // 模型缺失时两次都应立即失败：失败不写缓存，修好模型后下次可重试
        assert!(handle.get().await.is_err());
        assert!(handle.get().await.is_err());
        assert!(handle.loaded().is_none());
    }

    #[test]
    fn noop_engine_reports_error() {
        let engine = NoopOcr;
        assert_eq!(engine.name(), "none");
        let prepared = PreparedImage {
            bytes: vec![],
            mime: "image/png",
            width: 1,
            height: 1,
            sha256: "x".into(),
            phash: "0".repeat(16),
            profile: crate::vision::ImageProfile {
                width: 1,
                height: 1,
                format: "png".into(),
                has_alpha: false,
                animated: false,
                frames: 1,
                source_bytes: 0,
            },
            thumbnail: crate::vision::Thumbnail {
                bytes: vec![],
                mime: "image/png",
                ext: "png",
            },
        };
        assert!(engine.recognize(&prepared).is_err());
    }
}
