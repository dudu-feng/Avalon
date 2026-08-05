# 向量存储模块 (`loop/zvec_store.py`, `loop/embedding_service.py`)

## 概述

向量存储模块提供会话记忆的向量化存储与检索能力。由两个子模块组成：
- **EmbeddingService**：文本向量化服务（本地 SentenceTransformer 模型）
- **ZvecStore**：向量数据库操作封装（基于 ZVec）

## 文件清单

| 文件 | 说明 |
|------|------|
| `loop/embedding_service.py` | Embedding 向量化服务（单例，懒加载） |
| `loop/zvec_store.py` | ZVec 向量数据库 CRUD（单例） |

---

## embedding_service.py

### 核心类：`EmbeddingService`

**设计模式**：单例 + 懒加载（首次调用 embedding 方法时才加载模型）

**模型**：`bge-small-zh-v1.5`（512 维中文 Embedding 模型）

#### 方法

##### `doc_embedding(doc_text: str) -> np.ndarray`

**功能**：文档向量化（不带指令）。

**实现**：
1. 懒加载模型 `_init_model()`
2. 文本 strip + 空值校验
3. `model.encode(doc_text, normalize_embeddings=True)`

**调用方**：`zvec_store.insert_session_memory()`, `zvec_store.upsert_session_memory()`

---

##### `query_embedding(query_text: str) -> np.ndarray`

**功能**：检索 query 向量化（拼接指令前缀）。

**实现**：
1. 懒加载模型
2. 拼接指令：`"为这个句子生成表示以用于检索相关文章：" + query_text`
3. `model.encode(instruction + query_text, normalize_embeddings=True)`

> **注意**：bge 模型使用非对称检索，query 和 doc 的向量化方式不同。

**调用方**：`zvec_store.vectorQuery_session_memory()`, `zvec_store.hybridQuery_session_memory()`

---

##### `batch_doc_embedding(text_list: List[str]) -> np.ndarray`

**功能**：批量文档向量化，自动过滤空字符串。

---

##### `model` (property)

**功能**：只读属性，获取底层 SentenceTransformer 对象（触发懒加载）。

---

### 对外接口

```python
from loop.embedding_service import embedding_service

# 文档向量化
vec = embedding_service.doc_embedding("会话摘要文本")

# 查询向量化
query_vec = embedding_service.query_embedding("搜索关键词")

# 批量向量化
vecs = embedding_service.batch_doc_embedding(["文本1", "文本2"])
```

---

## zvec_store.py

### 核心类：`ZvecStore`

**设计模式**：单例，初始化时打开/创建向量数据库

#### Schema 定义

```python
CollectionSchema(
    name="Avalon_session_memory_index",
    fields=[
        FieldSchema(name="description",   STRING,        FTS(jieba)),  # 摘要文本 + 全文索引
        FieldSchema(name="keyWords",      ARRAY_STRING),               # 关键词列表
        FieldSchema(name="timestamp",     STRING),                     # 时间戳
    ],
    vectors=[
        VectorSchema(name="summary_vector", VECTOR_FP32, 512维, HNSW(COSINE)),  # 摘要向量
    ],
)
```

#### 初始化逻辑

1. 获取数据库路径 `env_config.zvec_db_path`
2. 若目录不存在 → `zvec.create_and_open(path, schema, option)` 创建新库
3. 若目录已存在 → `zvec.open(path, option)` 打开已有库
4. 选项：`read_only=False, enable_mmap=True`

---

#### 方法

##### `insert_session_memory(doc_id: str, text: str, keywords: list = None, timestamp: str = "")`

**功能**：插入会话记忆文档。

**实现**：
1. `embedding_service.doc_embedding(text)` 生成向量
2. 构建 `zvec.Doc(id, vectors, fields)`
3. `collection.insert(doc)`

**doc_id 格式**：
- 普通块：`{session_id}_chunk_{N}`
- 合并块：`{session_id}_chunk_merged_{start}_{end}`

**调用方**：`session_manage.session_compress()`, `session_manage._progressive_summarize()`

---

##### `upsert_session_memory(doc_id: str, text: str, keywords: list = None, timestamp: str = "")`

**功能**：更新或插入会话记忆（存在则覆盖，不存在则插入）。

---

##### `delete_session_memory(doc_id: str)`

**功能**：删除单个会话记忆。

**调用方**：`session_manage._progressive_summarize()`

---

##### `batch_delete_session_memory(doc_ids: list[str])`

**功能**：批量删除会话记忆。

---

##### `vectorQuery_session_memory(queryContent: str, topk: int = 5, filter_expr: str = "")`

**功能**：语义向量检索。

**实现**：
1. `embedding_service.query_embedding(queryContent)` 生成查询向量
2. `collection.query(field_name="summary_vector", vector=queryVector)`
3. 返回 topk 结果，包含 `description`、`keyWords`、`timestamp` 字段

**调用方**：`session_memory_tool.search_session_memory()` (semantic 模式)

---

##### `scalarQuery_session_memory(queryContent: str, topk: int = 5, filter_expr: str = "")`

**功能**：FTS 全文检索（基于 jieba 中文分词）。

**实现**：
1. `collection.query(field_name="description", fts=Fts(match_string=queryContent))`
2. 不需要向量化，直接文本匹配

**调用方**：`session_memory_tool.search_session_memory()` (keyword 模式)

---

##### `hybridQuery_session_memory(queryContent: str, topk: int = 5, filter_expr: str = "")`

**功能**：混合检索（语义向量 + FTS 全文检索）。

**实现**：
1. 生成查询向量
2. `collection.query(field_name="summary_vector", vector=queryVector, fts=Fts(match_string=queryContent))`
3. ZVec 内部融合两种信号返回综合排序

**调用方**：`session_memory_tool.search_session_memory()` (hybrid 模式，默认)

---

##### `collection` (property)

**功能**：只读暴露底层 ZVec collection 对象。

---

### 对外接口

```python
from loop.zvec_store import zvec_store

# 插入
zvec_store.insert_session_memory("terminal_xxx_chunk_1", "摘要文本", ["关键词"], "2026-06-06")

# 语义检索
results = zvec_store.vectorQuery_session_memory("压缩方案", topk=5)

# 全文检索
results = zvec_store.scalarQuery_session_memory("Avalon", topk=5)

# 混合检索
results = zvec_store.hybridQuery_session_memory("Avalon 压缩", topk=5)

# 删除
zvec_store.delete_session_memory("terminal_xxx_chunk_1")
```

---

## 依赖关系

| embedding_service 依赖 | 用途 |
|------------------------|------|
| `sentence_transformers` | 本地 Embedding 模型 |
| `numpy` | 向量数据类型 |
| `config.env_config` | 模型路径、设备配置 |

| zvec_store 依赖 | 用途 |
|-----------------|------|
| `zvec` | 向量数据库 SDK |
| `loop.embedding_service` | 文本向量化 |
| `config.env_config` | 数据库路径 |

## Tauri 迁移要点

### EmbeddingService
- Rust 中使用 `candle-core` 或 `ort` (ONNX Runtime) 加载 bge 模型
- 512 维向量用 `Vec<f32>` 或 `ndarray::Array1<f32>` 表示
- `normalize_embeddings=True` 需手动实现 L2 归一化
- query 指令前缀拼接保持一致
- 考虑模型加载耗时，需懒加载 + 缓存

### ZvecStore
- ZVec 有 Rust 原生 SDK，可直接使用
- Schema 定义需要转换为 Rust struct
- HNSW + COSINE 索引参数保持一致
- FTS jieba 分词器需确认 Rust 版 ZVec 是否支持
- 单例模式用 `OnceLock<ZvecStore>` 实现
