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

/// 加载句柄：OnceCell 保证「只加载一次」，之后 Arc 常驻复用（lazy/eager 共用）。
/// lazy 与 eager 的区别仅在第一次 load 发生在首次使用时还是启动时。
pub struct EmbedderHandle {
    cell: tokio::sync::OnceCell<Arc<dyn Embedder>>,
    config: AppConfig,
}

impl EmbedderHandle {
    pub fn new(config: AppConfig) -> Self {
        Self {
            cell: tokio::sync::OnceCell::new(),
            config,
        }
    }

    /// 常驻热加载：启动时后台预热（spawn_blocking，不阻塞主线程）
    pub async fn warmup(&self) -> Result<Arc<dyn Embedder>> {
        self.get().await
    }

    /// 懒加载：首次调用才真正 load；OnceCell 保证只加载一次，之后都是热启动（复用 Arc）
    pub async fn get(&self) -> Result<Arc<dyn Embedder>> {
        self.cell
            .get_or_try_init(|| async {
                let config = self.config.clone();
                tokio::task::spawn_blocking(move || build(&config))
                    .await
                    .map_err(|e| anyhow!("embedding 加载任务失败: {e}"))?
            })
            .await
            .map(|a| a.clone())
    }
}
