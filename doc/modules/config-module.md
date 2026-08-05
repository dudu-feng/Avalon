# 配置模块 (`config/`)

## 概述

配置模块是整个系统的配置入口，通过单例模式统一管理所有环境变量。其他模块不直接调用 `load_dotenv()`，而是通过 `env_config` 实例获取配置。

## 文件清单

| 文件 | 说明 |
|------|------|
| `config/env_config.py` | 环境变量配置类（单例） |

## 核心类：`EnvConfig`

### 设计模式
- **单例模式**：通过 `__new__` + `_initialized` 标志确保全局唯一实例
- **属性暴露**：所有配置通过 `@property` 暴露，调用方不关心变量名和默认值
- **懒加载**：首次实例化时才加载 `.env` 文件

### 实现逻辑

```python
class EnvConfig:
    _instance: Optional["EnvConfig"] = None  # 单例缓存

    def __new__(cls):
        # 首次创建时标记未初始化
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance._initialized = False
        return cls._instance

    def __init__(self):
        # 已初始化则跳过，避免重复加载
        if self._initialized:
            return
        self._load_dotenv()
        self._initialized = True
```

`_load_dotenv()` 从 `agent/` 目录下查找 `.env` 文件（基于当前文件位置向上推导，不依赖工作目录）。

## 配置项清单

### LLM 配置

| 属性 | 环境变量 | 类型 | 说明 |
|------|----------|------|------|
| `default_api_key` | `default_api_key` | str | LLM API Key |
| `default_model` | `default_model` | str | 模型名称 |
| `default_model_base_url` | `default_model_base_url` | str | API 基础 URL |

### 路径配置

| 属性 | 环境变量 | 类型 | 说明 |
|------|----------|------|------|
| `prompt_file_path` | `prompt_file_path` | str | 提示词文件目录 |
| `memory_path` | `memory_path` | str | 记忆存储根目录 |
| `session_path` | `session_path` | str | 会话存储目录 |
| `session_index_path` | `session_index_path` | str | 会话索引文件 |

### 向量数据库配置

| 属性 | 环境变量 | 类型 | 说明 |
|------|----------|------|------|
| `vector_db_path` | `vector_db_path` | str | 向量库根目录 |
| `model_cache_dir` | `model_cache_dir` | str | 模型缓存目录 |

### Embedding 配置

| 属性 | 环境变量 | 类型 | 默认值 | 说明 |
|------|----------|------|--------|------|
| `embedding_mode` | `embedding_mode` | str | `"local"` | `local`/`api` |
| `local_embedding_model` | `local_embedding_model` | str | - | 本地模型名 |
| `embedding_device` | `embedding_device` | str | `"cpu"` | `cpu`/`cuda` |
| `api_embedding_key` | `api_embedding_key` | str | - | API Key |
| `api_embedding_model` | `api_embedding_model` | str | - | API 模型名 |
| `api_embedding_base_url` | `api_embedding_base_url` | str | - | API URL |

### 派生属性（计算得出）

| 属性 | 计算逻辑 | 说明 |
|------|----------|------|
| `local_embedding_model_path` | `model_cache_dir + local_embedding_model` | 本地模型完整路径 |
| `chroma_db_path` | `vector_db_path + "/chroma"` | ChromaDB 路径（预留） |
| `zvec_db_path` | `vector_db_path + "/zvec"` | ZVec 持久化路径 |

### 会话压缩配置

| 属性 | 环境变量 | 类型 | 默认值 | 说明 |
|------|----------|------|--------|------|
| `session_memory_compress_threshold` | `session_memory_compress_threshold` | int | 20000 | 自动压缩 token 阈值，支持 `"512k"` 格式 |
| `session_memory_max_chunks` | `session_memory_max_chunks` | int | 10 | 触发渐进式总结的压缩块上限 |
| `session_memory_context_chunks` | `session_memory_context_chunks` | int | 5 | 系统提示加载的最近压缩块数 |

## 对外接口

```python
# 全局实例，其他模块直接导入使用
from config.env_config import env_config

# 使用示例
api_key = env_config.default_api_key
threshold = env_config.session_memory_compress_threshold
zvec_path = env_config.zvec_db_path
```

## Tauri 迁移要点

- Rust 中用 `std::sync::OnceLock` 或 `lazy_static!` 实现单例
- `.env` 替换为 Tauri 的配置文件（`tauri.conf.json` 或自定义 JSON/TOML）
- 路径配置需适配 Tauri 的 `app_data_dir()` / `app_config_dir()`
- 阈值解析逻辑（`"512k"` → int）需在 Rust 中重新实现
