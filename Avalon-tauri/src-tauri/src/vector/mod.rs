// 向量数据库模块
//
// 职责：会话记忆的向量存储与检索（semantic / keyword / hybrid）。
// 定义 VectorStore（存储）+ MemoryIndex（检索）契约，InMemoryStore 为自研轻量实现，
// trait 抽象预留 sqlite 扩展（config.vector.backend 抉择）。
// 依赖 embedding 的 Embedder 做文本编码（依赖反转：消费 trait，不关心模型）。

#![allow(dead_code)] // 供未来 session/tool 引用，当前无调用方，接入后移除
#![allow(unused_imports)] // MemoryDoc 等 re-export 供外部模块使用，mod 内暂未引用

pub mod doc;
pub mod memory;
pub mod search;

use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::config::{AppConfig, SearchMode, VectorBackend};
use crate::embedding::EmbedderHandle;

pub use doc::{MemoryDoc, MemoryHit, RebuildProgress, RebuildStats};
pub use memory::InMemoryStore;

/// 检索契约：检索工具（04 search_session_memory）依赖，只读语义
pub trait MemoryIndex: Send + Sync {
    /// 三模式检索，topk 结果按分数降序
    fn search(
        &self,
        query: &str,
        mode: SearchMode,
        topk: usize,
        time_range: &str,
    ) -> Result<Vec<MemoryHit>>;
    /// 清空索引（扫描 session 文件的重建编排在 session 层）
    fn rebuild(&self) -> Result<RebuildStats>;
}

/// 存储契约：session 模块（未来）写入依赖，MemoryIndex 的超集
pub trait VectorStore: MemoryIndex {
    fn insert(
        &self,
        doc_id: &str,
        text: &str,
        keywords: &[String],
        timestamp: &str,
    ) -> Result<()>;
    fn upsert(
        &self,
        doc_id: &str,
        text: &str,
        keywords: &[String],
        timestamp: &str,
    ) -> Result<()>;
    fn delete(&self, doc_id: &str) -> Result<()>;
    fn batch_delete(&self, doc_ids: &[String]) -> Result<()>;
    /// 当前记录数
    fn len(&self) -> usize;
}

/// 按 config.vector.backend 构造向量库，同时返回两个视角：
/// `Arc<dyn VectorStore>` 供 session 写入，`Arc<dyn MemoryIndex>` 供 tool 检索（只读）。
/// 二者共享同一底层实例（同源 Arc，仅 trait 视角收窄）。
pub fn build(
    config: &AppConfig,
    handle: EmbedderHandle,
) -> Result<(Arc<dyn VectorStore>, Arc<dyn MemoryIndex>)> {
    match config.vector.backend {
        VectorBackend::Memory => {
            let path = config.vector_db_path().join("memory.bin");
            let store: Arc<InMemoryStore> = Arc::new(InMemoryStore::open(&path, handle)?);
            let memory: Arc<dyn MemoryIndex> = store.clone();
            let vector: Arc<dyn VectorStore> = store;
            Ok((vector, memory))
        }
        VectorBackend::Sqlite => Err(anyhow!("sqlite 后端尚未实现（预留扩展）")),
    }
}
