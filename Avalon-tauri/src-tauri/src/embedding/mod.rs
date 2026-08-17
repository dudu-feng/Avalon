// Embedding 向量模型模块
//
// 职责：本地 bge 模型的文本 → 向量推理。定义 Embedder 契约（依赖反转：
// 未来 vector/session 依赖本模块的 trait），LocalEmbedder 用 candle 纯 Rust 推理。
// 向量库存储/检索（ZvecStore 替换）不在此模块，单独后置设计。

#![allow(dead_code)] // embedding 模块供未来 vector/session 引用，当前无调用方，接入后移除

pub mod local;

use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::config::{AppConfig, EmbeddingMode};

pub use local::LocalEmbedder;

/// 向量化契约：文本 → 归一化稠密向量。消费方通过 trait object 注入，不关心具体模型。
pub trait Embedder: Send + Sync {
    /// 输出向量维度（bge-small=512 / bge-large=1024），供下游 schema 动态对齐
    fn dim(&self) -> usize;
    /// 文档向量：裸文本，不带检索指令
    fn doc_embedding(&self, text: &str) -> Result<Vec<f32>>;
    /// 查询向量：拼接 BGE 检索指令前缀（不对称检索）
    fn query_embedding(&self, text: &str) -> Result<Vec<f32>>;
    /// 批量文档向量：自动过滤空/空白串
    fn batch_doc_embedding(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// 按 config.embedding.mode 构造 embedder
pub fn build(config: &AppConfig) -> Result<Arc<dyn Embedder>> {
    match config.embedding.mode {
        EmbeddingMode::Local => {
            let path = config.local_embedding_model_path();
            let device = config.embedding.device.clone();
            Ok(Arc::new(LocalEmbedder::load(&path, &device)?))
        }
        EmbeddingMode::Api => Err(anyhow!("api embedding 尚未实现")),
    }
}

/// 加载句柄：OnceLock 保证「只加载一次」，之后 Arc 常驻复用（lazy/eager 共用）。
/// lazy 与 eager 的区别仅在第一次 load 发生在首次使用时还是启动时。
/// 决策甲：std::sync::OnceLock + get_sync，供 vector/session 的同步 trait 内部拿 embedder。
#[derive(Clone)]
pub struct EmbedderHandle {
    cell: Arc<std::sync::OnceLock<Arc<dyn Embedder>>>,
    /// 懒加载构建器（from_embedder 直接注入实例时为 None，cell 已预填）
    config: Option<AppConfig>,
}

impl EmbedderHandle {
    /// 由 config 构建：embedder 首次使用时才真正 load（load_mode 决定 eager 是否启动预热）
    pub fn new(config: AppConfig) -> Self {
        Self {
            cell: Arc::new(std::sync::OnceLock::new()),
            config: Some(config),
        }
    }

    /// 直接注入已构造的 embedder（测试注入 mock / 复用现成实例）：OnceLock 预填，get_sync 秒回
    pub fn from_embedder(embedder: Arc<dyn Embedder>) -> Self {
        let cell = Arc::new(std::sync::OnceLock::new());
        let _ = cell.set(embedder);
        Self { cell, config: None }
    }

    /// 同步获取：首次调用同步加载（阻塞秒级，仅一次），之后热启动复用。
    /// 供 vector/session 的同步 trait 内部使用（决策甲：trait 保持同步，不 async 化）。
    /// 手动 get/set 而非 get_or_try_init（后者为 unstable once_cell_try）：失败不缓存，并发时后到者复用先到者实例。
    pub fn get_sync(&self) -> Result<Arc<dyn Embedder>> {
        if let Some(embedder) = self.cell.get() {
            return Ok(embedder.clone());
        }
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| anyhow!("EmbedderHandle 已注入实例，无 config 构建器"))?;
        let embedder = build(config)?;
        match self.cell.set(embedder.clone()) {
            Ok(()) => Ok(embedder),
            // set 失败说明已被其他线程先初始化，改用既有实例
            Err(_) => Ok(self.cell.get().expect("set 失败说明 cell 已初始化").clone()),
        }
    }

    /// 懒加载：已加载则秒回；未加载在 spawn_blocking 里加载，不阻塞 async worker
    pub async fn get(&self) -> Result<Arc<dyn Embedder>> {
        if let Some(embedder) = self.cell.get() {
            return Ok(embedder.clone());
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.get_sync())
            .await
            .map_err(|e| anyhow!("embedding 加载任务失败: {e}"))?
    }

    /// 常驻热加载：启动时后台预热（spawn_blocking，不阻塞主线程）
    pub async fn warmup(&self) -> Result<Arc<dyn Embedder>> {
        self.get().await
    }
}
