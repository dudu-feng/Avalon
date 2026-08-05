# 对外接口参考

> 本文档汇总各模块对外开放的函数、类、属性及其签名，供 Tauri 迁移时参照实现。

## 1. 配置模块 (`config/env_config.py`)

### 全局实例

```python
env_config = EnvConfig()  # 全局唯一单例
```

### 属性接口

| 属性 | 返回类型 | 说明 |
|------|----------|------|
| `default_api_key` | `str` | LLM API Key |
| `default_model` | `str` | 模型名称 |
| `default_model_base_url` | `str` | API 基础 URL |
| `prompt_file_path` | `str` | 提示词文件目录 |
| `memory_path` | `str` | 记忆存储根目录 |
| `session_path` | `str` | 会话存储目录 |
| `session_index_path` | `str` | 会话索引文件路径 |
| `vector_db_path` | `str` | 向量库根目录 |
| `model_cache_dir` | `str` | 模型缓存目录 |
| `embedding_mode` | `str` | `local` / `api` |
| `local_embedding_model` | `str` | 本地模型名 |
| `embedding_device` | `str` | `cpu` / `cuda` |
| `api_embedding_key` | `str` | API Embedding Key |
| `api_embedding_model` | `str` | API 模型名 |
| `api_embedding_base_url` | `str` | API URL |
| `local_embedding_model_path` | `str` | 本地模型完整路径（派生） |
| `chroma_db_path` | `str` | ChromaDB 路径（派生） |
| `zvec_db_path` | `str` | ZVec 路径（派生） |
| `session_memory_compress_threshold` | `int` | 自动压缩 token 阈值 |
| `session_memory_max_chunks` | `int` | 渐进式总结触发上限 |
| `session_memory_context_chunks` | `int` | 系统提示加载压缩块数 |

---

## 2. LLM 交互模块 (`llm/llm.py`)

### 函数接口

```python
def get_model() -> ChatOpenAI
```
创建标准 ChatOpenAI 客户端。

```python
def get_jsonOutput_model() -> ChatOpenAI
```
创建强制 JSON 输出的 ChatOpenAI 客户端。

```python
def llm_chat(user_input: str, chat_history: list) -> BaseMessage
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `user_input` | `str` | 当前用户输入 |
| `chat_history` | `list` | 当前轮聊天历史 |
| **返回** | `BaseMessage` | LLM 响应（`.content` 为 JSON 字符串） |

```python
def llm_action(user_input: str, action_target: str, action_history: list) -> BaseMessage
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `user_input` | `str` | 原始用户输入 |
| `action_target` | `str` | 动作目标描述 |
| `action_history` | `list` | 已执行的操作历史 |
| **返回** | `BaseMessage` | LLM 响应（JSON，含 `next`/`tool_call`/`sub_analysis`） |

```python
def llm_compress(session_data: dict) -> BaseMessage
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `session_data` | `dict` | 待压缩的会话数据 |
| **返回** | `BaseMessage` | LLM 响应（JSON，含 `summary`/`keywords`） |

```python
def change_model_type(type: str) -> None
```
切换模型类型（预留）。

---

## 3. 核心循环模块 (`loop/react_loop.py`, `loop/prompt_assemble.py`)

### react_loop.py

```python
def react_loop(user_input: str) -> dict
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `user_input` | `str` | 用户输入 |
| **返回** | `list[dict]` | 完整聊天历史（含 user/assistant/action 记录） |

```python
def execute_tool(tool_name: str, arguments: dict) -> str
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `tool_name` | `str` | 工具名称 |
| `arguments` | `dict` | 工具参数 |
| **返回** | `str` | 工具执行结果 |

```python
def parse_llm_json(llm_output_content: str) -> dict
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `llm_output_content` | `str` | LLM 输出内容 |
| **返回** | `dict` | 解析后的 dict，失败返回 `{}` |

```python
def chat_result_transform(chat_result_content: dict, chat_result) -> dict
def action_result_transform(action_result_content: dict, action_result) -> dict
def get_current_time() -> str  # "%Y-%m-%d-%H:%M:%S"
```

### prompt_assemble.py

```python
def assemble_system_prompt() -> list
```
| **返回** | `list[str]` | 系统提示词字符串列表 |

```python
def load_prompt(file_name: str) -> str
```
| 参数 | 类型 | 说明 |
|------|------|------|
| `file_name` | `str` | 提示词文件名 |
| **返回** | `str` | 文件内容 |

---

## 4. 会话管理模块 (`loop/session_manage.py`)

```python
def init_session() -> None
```
初始化或恢复当前会话。

```python
def get_current_session() -> dict
```
读取当前会话完整数据。

```python
def update_current_session(chat_history: list) -> None
```
追加聊天历史到当前会话。

```python
def save_current_session() -> None
```
归档当前会话到历史目录。

```python
def session_compress() -> None
```
压缩当前会话（LLM 压缩 + ZVec 同步 + 渐进式总结）。

```python
def auto_compress_check_from_history(chat_history: list) -> bool
```
| **返回** | `bool` | `True` 已触发压缩，`False` 未达阈值 |

```python
def get_session_context_for_prompt() -> dict
```
| **返回** | `dict` | 裁剪后的会话上下文（super_compressed + 最近N个compressed） |

```python
def _progressive_summarize() -> None  # 内部函数
```
渐进式总结，合并旧压缩块为超级摘要。

---

## 5. 向量存储模块

### embedding_service.py

```python
embedding_service = EmbeddingService()  # 全局单例
```

| 方法 | 签名 | 返回 | 说明 |
|------|------|------|------|
| `doc_embedding` | `(doc_text: str)` | `np.ndarray` | 文档向量化（不带指令） |
| `query_embedding` | `(query_text: str)` | `np.ndarray` | 查询向量化（带指令前缀） |
| `batch_doc_embedding` | `(text_list: List[str])` | `np.ndarray` | 批量文档向量化 |
| `model` (property) | - | `SentenceTransformer` | 底层模型对象 |

### zvec_store.py

```python
zvec_store = ZvecStore()  # 全局单例
```

| 方法 | 签名 | 返回 | 说明 |
|------|------|------|------|
| `insert_session_memory` | `(doc_id, text, keywords?, timestamp?)` | result | 插入记忆 |
| `upsert_session_memory` | `(doc_id, text, keywords?, timestamp?)` | result | 更新或插入 |
| `delete_session_memory` | `(doc_id)` | result | 删除单个 |
| `batch_delete_session_memory` | `(doc_ids)` | result | 批量删除 |
| `vectorQuery_session_memory` | `(queryContent, topk?, filter_expr?)` | results | 语义检索 |
| `scalarQuery_session_memory` | `(queryContent, topk?, filter_expr?)` | results | 全文检索 |
| `hybridQuery_session_memory` | `(queryContent, topk?, filter_expr?)` | results | 混合检索 |
| `collection` (property) | - | ZVec Collection | 底层集合对象 |

---

## 6. 工具模块

### base_tool.py

```python
TOOLS = [read_file, write_file, delete_file,
         run_shell_command, get_directory_contents,
         search_session_memory]
```

| 工具函数 | 签名 | 返回 |
|----------|------|------|
| `read_file` | `(file_path: str)` | `str` |
| `write_file` | `(file_path: str, content: str)` | `str` |
| `delete_file` | `(file_path: str)` | `str` |
| `run_shell_command` | `(command: str)` | `str` |
| `get_directory_contents` | `(directory_path: str)` | `str` |
| `get_tool_list()` | `()` | `str` — 格式化工具列表 |

### session_memory_tool.py

```python
@tool
def search_session_memory(query: str, search_mode: str = "hybrid",
                          topk: int = 5, time_range: str = "") -> str
```
| **返回** | `str` | JSON 格式的检索结果数组 |

```python
def _build_time_filter(time_range: str) -> str   # 内部函数
def _parse_doc_id(doc_id: str) -> tuple           # 内部函数
```

---

## 7. 入口模块 (`main.py`)

```python
# 启动流程
session_manage.init_session()

# 主循环
chat_history = react_loop.react_loop(user_input)
session_manage.update_current_session(chat_history)
session_manage.auto_compress_check_from_history(chat_history)

# 退出
session_manage.save_current_session()
```

### 支持的命令

| 命令 | 说明 |
|------|------|
| `/compress` | 手动触发会话压缩 |
| `/exit`, `/quit` | 退出程序 |
| `/help` | 显示帮助 |
