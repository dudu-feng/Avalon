// InMemoryStore：自研轻量内存索引 + bincode 持久化
//
// 存储布局：单一 RwLock<Index> 保护三份状态（docs + 倒排 + 文档长度），
// 规避多锁交叉的死锁风险。检索走 search.rs 纯函数；写入维护三态并全量落盘。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};

use crate::config::SearchMode;
use crate::embedding::Embedder;

use super::doc::{MemoryDoc, MemoryHit, RebuildStats};
use super::search;
use super::{MemoryIndex, VectorStore};

/// 内存索引的完整状态（单锁保护，读写一致）
struct Index {
    /// doc_id → MemoryDoc
    docs: HashMap<String, MemoryDoc>,
    /// 倒排索引：bigram → (doc_id → tf)
    inverted: HashMap<String, HashMap<String, u32>>,
    /// doc_id → bigram 数量（BM25 文档长度）
    doc_len: HashMap<String, usize>,
}

impl Index {
    fn new() -> Self {
        Self {
            docs: HashMap::new(),
            inverted: HashMap::new(),
            doc_len: HashMap::new(),
        }
    }

    fn clear(&mut self) {
        self.docs.clear();
        self.inverted.clear();
        self.doc_len.clear();
    }

    /// 平均文档长度（BM25 归一化）
    fn avgdl(&self) -> f32 {
        if self.doc_len.is_empty() {
            1.0
        } else {
            self.doc_len.values().sum::<usize>() as f32 / self.doc_len.len() as f32
        }
    }
}

/// 从索引中移除某 doc（清理其倒排贡献 + 文档长度），返回是否真的移除过
fn remove_from_index(index: &mut Index, doc_id: &str) {
    if let Some(doc) = index.docs.remove(doc_id) {
        for term in search::bigrams(&doc.description) {
            if let Some(posting) = index.inverted.get_mut(&term) {
                posting.remove(doc_id);
                if posting.is_empty() {
                    index.inverted.remove(&term);
                }
            }
        }
        index.doc_len.remove(doc_id);
    }
}

/// 自研轻量向量库（重资源仅 embedder，由调用方 Arc 注入；索引本身轻量）
pub struct InMemoryStore {
    index: RwLock<Index>,
    /// 持久化文件路径
    path: PathBuf,
    /// 检索/入库编码器（semantic/hybrid 时 query_embedding，insert 时 doc_embedding）
    embedder: Arc<dyn Embedder>,
}

impl InMemoryStore {
    /// 打开（存在则加载，不存在则空），对齐 Python「已存在 open / 不存在 create」
    pub fn open(path: &Path, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let store = Self {
            index: RwLock::new(Index::new()),
            path: path.to_path_buf(),
            embedder,
        };
        store.load()?;
        Ok(store)
    }

    /// 从持久化文件加载 docs，并重建倒排索引
    fn load(&self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let bytes = std::fs::read(&self.path)
            .with_context(|| format!("读取向量库文件失败: {}", self.path.display()))?;
        let docs: HashMap<String, MemoryDoc> = bincode::deserialize(&bytes)
            .with_context(|| format!("解析向量库文件失败: {}", self.path.display()))?;

        let mut index = self.index.write().unwrap();
        index.inverted.clear();
        index.doc_len.clear();
        // 遍历反序列化得到的 docs（独立局部变量），同时可变借用 index 的倒排/长度字段，无借用冲突
        for (id, doc) in &docs {
            let terms = search::bigrams(&doc.description);
            index.doc_len.insert(id.clone(), terms.len());
            for term in terms {
                let posting = index.inverted.entry(term).or_default();
                *posting.entry(id.clone()).or_insert(0) += 1;
            }
        }
        index.docs = docs;
        Ok(())
    }

    /// 全量序列化写回（tmp + rename 原子写）
    fn persist(&self) -> Result<()> {
        let bytes = {
            let index = self.index.read().unwrap();
            bincode::serialize(&index.docs).context("序列化向量库失败")?
        };
        let tmp = self.path.with_extension("bin.tmp");
        std::fs::write(&tmp, &bytes)
            .with_context(|| format!("写入向量库临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("原子替换向量库文件失败: {}", self.path.display()))?;
        Ok(())
    }

    /// 写入文档（insert / upsert 共用，覆盖写语义）
    fn write_doc(&self, doc: MemoryDoc) -> Result<()> {
        let terms = search::bigrams(&doc.description);
        let doc_len = terms.len();
        let id = doc.id.clone();
        {
            let mut index = self.index.write().unwrap();
            remove_from_index(&mut index, &id); // 覆盖写：先清旧索引
            for term in terms {
                let posting = index.inverted.entry(term).or_default();
                *posting.entry(id.clone()).or_insert(0) += 1;
            }
            index.doc_len.insert(id.clone(), doc_len);
            index.docs.insert(id, doc);
        }
        self.persist()
    }

    /// 三模式检索：锁外编码 query，锁内计算排名 + 时间过滤 + 取 topk
    fn search_impl(
        &self,
        query: &str,
        mode: SearchMode,
        topk: usize,
        time_range: &str,
    ) -> Result<Vec<MemoryHit>> {
        let topk = topk.clamp(1, 20);
        let (start, end) = search::parse_time_range(time_range);

        // 需向量编码的模式，先在锁外编码（candle 推理 CPU 密集，不持锁）
        let query_vec = match mode {
            SearchMode::Semantic | SearchMode::Hybrid => {
                Some(self.embedder.query_embedding(query)?)
            }
            SearchMode::Keyword => None,
        };

        let index = self.index.read().unwrap();
        let ranked: Vec<(String, f32)> = match mode {
            SearchMode::Semantic => search::semantic_rank(
                query_vec.as_ref().expect("semantic 必有 query_vec"),
                &index.docs,
            ),
            SearchMode::Keyword => search::keyword_rank(
                query,
                &index.inverted,
                &index.doc_len,
                index.avgdl(),
                index.docs.len(),
            ),
            SearchMode::Hybrid => {
                let semantic = search::semantic_rank(
                    query_vec.as_ref().expect("hybrid 必有 query_vec"),
                    &index.docs,
                );
                let keyword = search::keyword_rank(
                    query,
                    &index.inverted,
                    &index.doc_len,
                    index.avgdl(),
                    index.docs.len(),
                );
                search::rrf_fuse(&semantic, &keyword, 60.0)
            }
        };

        // 时间过滤 + topk + 组装 MemoryHit
        let mut hits: Vec<MemoryHit> = Vec::with_capacity(topk);
        for (id, score) in ranked {
            if hits.len() >= topk {
                break;
            }
            if let Some(doc) = index.docs.get(&id) {
                if search::in_range(&doc.timestamp, &start, &end) {
                    hits.push(MemoryHit {
                        doc_id: id,
                        description: doc.description.clone(),
                        keywords: doc.keywords.clone(),
                        timestamp: doc.timestamp.clone(),
                        score,
                    });
                }
            }
        }
        Ok(hits)
    }
}

impl MemoryIndex for InMemoryStore {
    fn search(
        &self,
        query: &str,
        mode: SearchMode,
        topk: usize,
        time_range: &str,
    ) -> Result<Vec<MemoryHit>> {
        self.search_impl(query, mode, topk, time_range)
    }

    fn rebuild(&self) -> Result<RebuildStats> {
        // 清空内存索引 + 删除持久化文件，回到空集合（扫描 session 文件的重建编排在 session 层）
        self.index.write().unwrap().clear();
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .with_context(|| format!("删除向量库文件失败: {}", self.path.display()))?;
        }
        Ok(RebuildStats::default())
    }
}

impl VectorStore for InMemoryStore {
    fn insert(
        &self,
        doc_id: &str,
        text: &str,
        keywords: &[String],
        timestamp: &str,
    ) -> Result<()> {
        let vector = self.embedder.doc_embedding(text)?;
        self.write_doc(MemoryDoc {
            id: doc_id.to_string(),
            description: text.to_string(),
            keywords: keywords.to_vec(),
            timestamp: timestamp.to_string(),
            vector,
        })
    }

    fn upsert(
        &self,
        doc_id: &str,
        text: &str,
        keywords: &[String],
        timestamp: &str,
    ) -> Result<()> {
        // 覆盖写语义与 insert 一致（write_doc 内部先清旧索引）
        self.insert(doc_id, text, keywords, timestamp)
    }

    fn delete(&self, doc_id: &str) -> Result<()> {
        {
            let mut index = self.index.write().unwrap();
            remove_from_index(&mut index, doc_id);
        }
        self.persist()
    }

    fn batch_delete(&self, doc_ids: &[String]) -> Result<()> {
        {
            let mut index = self.index.write().unwrap();
            for id in doc_ids {
                remove_from_index(&mut index, id);
            }
        }
        self.persist()
    }

    fn len(&self) -> usize {
        self.index.read().unwrap().docs.len()
    }
}
