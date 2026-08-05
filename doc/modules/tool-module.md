# 工具模块 (`tool/`)

## 概述

工具模块定义智能体可调用的所有工具。使用 LangChain 的 `@tool` 装饰器将 Python 函数注册为工具，供动作层 LLM 调用。分为基础工具（文件操作/Shell）和会话记忆检索工具。

## 文件清单

| 文件 | 说明 |
|------|------|
| `tool/base_tool.py` | 基础工具定义 + 工具注册表 |
| `tool/session_memory_tool.py` | 会话记忆检索工具 |

---

## base_tool.py

### 已注册工具

| 工具名 | 函数 | 参数 | 风险等级 | 说明 |
|--------|------|------|----------|------|
| `read_file` | `read_file` | `file_path: str` | 只读 | 读取文件内容 |
| `write_file` | `write_file` | `file_path: str, content: str` | 写入 | 创建或覆盖文件 |
| `delete_file` | `delete_file` | `file_path: str` | 危险 | 删除文件 |
| `run_shell_command` | `run_shell_command` | `command: str` | 危险 | 执行 Shell 命令（30s 超时） |
| `get_directory_contents` | `get_directory_contents` | `directory_path: str` | 只读 | 列出目录内容 |
| `search_session_memory` | `search_session_memory` | `query, search_mode, topk, time_range` | 只读 | 检索历史会话记忆 |

### 工具函数详解

#### `read_file(file_path: str) -> str`

**功能**：读取指定文件内容。

**实现**：`open(file_path, 'r', encoding='utf-8')` 读取全文，异常返回错误信息。

---

#### `write_file(file_path: str, content: str) -> str`

**功能**：创建或覆盖写入文件。

**实现**：`open(file_path, 'w', encoding='utf-8')` 写入内容，返回成功消息。

---

#### `delete_file(file_path: str) -> str`

**功能**：删除指定文件。

**实现**：`os.remove(file_path)`，返回成功/错误消息。

---

#### `run_shell_command(command: str) -> str`

**功能**：在终端执行命令，返回标准输出和错误。

**实现**：
```python
subprocess.run(command, shell=True, capture_output=True, text=True, timeout=30)
return result.stdout + result.stderr
```

- **超时**：30 秒
- **Shell 模式**：`shell=True`，支持管道等 Shell 特性

---

#### `get_directory_contents(directory_path: str) -> str`

**功能**：获取目录下所有文件和子目录。

**实现**：`os.listdir(directory_path)` 返回列表。

---

### 工具注册表

```python
TOOLS = [read_file, write_file, delete_file,
         run_shell_command, get_directory_contents,
         search_session_memory]
```

`TOOLS` 是全局列表，`react_loop.execute_tool()` 遍历此列表匹配工具名。

---

### `get_tool_list() -> str`

**功能**：生成格式化的工具列表字符串，供 LLM 系统提示使用。

**实现**：遍历 TOOLS，拼接 `"- **{name}**: {description}"`，返回 Markdown 格式字符串。

**调用方**：`llm.llm_chat()`, `llm.llm_action()`

---

## session_memory_tool.py

### `search_session_memory(query, search_mode, topk, time_range) -> str`

**功能**：搜索历史会话记忆。在压缩后的会话摘要中检索与 query 最相关的内容。

**LangChain @tool 装饰器**：自动从 docstring 生成工具描述和参数说明。

#### 参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `query` | str | - | 搜索文本（自然语言或关键词） |
| `search_mode` | str | `"hybrid"` | `semantic` / `keyword` / `hybrid` |
| `topk` | int | 5 | 返回结果数量（1-20） |
| `time_range` | str | `""` | 时间过滤（`"2026-07-15"` 或 `"2026-07-01,2026-07-31"`） |

#### 实现逻辑

1. **参数校验**：`search_mode` 必须是 `semantic`/`keyword`/`hybrid`
2. **topk 裁剪**：`max(1, min(topk, 20))`
3. **构建时间过滤**：`_build_time_filter(time_range)`
4. **选择查询方法**：
   - `semantic` → `zvec_store.vectorQuery_session_memory()`
   - `keyword` → `zvec_store.scalarQuery_session_memory()`
   - `hybrid` → `zvec_store.hybridQuery_session_memory()`
5. **格式化结果**：每条结果转为 dict，包含 `location`、`session_id`、`chunk`、`description`、`keywords`、`timestamp`、`score`
6. **返回 JSON 字符串**

#### 返回格式

```json
[
  {
    "location": "/path/to/session/history/terminal/terminal_xxx/raw/2.json",
    "session_id": "terminal_2026-06-06-23_03_31",
    "chunk": 2,
    "description": "会话摘要文本",
    "keywords": ["关键词1", "关键词2"],
    "timestamp": "2026-06-06-23_03_31",
    "score": 0.8523
  }
]
```

- `location`：源会话片段的绝对路径，LLM 可用 `read_file` 读取原文
- `score`：相关度分数（越高越相关）

---

### `_build_time_filter(time_range: str) -> str`

**功能**：将用户输入的时间范围转为 ZVec filter 表达式。

| 输入 | 输出 |
|------|------|
| `""` | `""` （无过滤） |
| `"2026-07-15"` | `timestamp >= "2026-07-15-00_00_00"` |
| `"2026-07-01,2026-07-31"` | `timestamp >= "2026-07-01-00_00_00" && timestamp <= "2026-07-31-23_59_59"` |

---

### `_parse_doc_id(doc_id: str) -> tuple`

**功能**：从 doc_id 中解析会话 ID 和 chunk 标识。

| doc_id 格式 | 返回 |
|-------------|------|
| `terminal_xxx_chunk_2` | `("terminal_xxx", 2)` |
| `terminal_xxx_chunk_merged_1_5` | `("terminal_xxx", "merged_1_5")` |

---

## 依赖关系

| base_tool 依赖 | 用途 |
|----------------|------|
| `langchain_core.tools.tool` | @tool 装饰器 |
| `tool.session_memory_tool` | 导入搜索工具 |

| session_memory_tool 依赖 | 用途 |
|--------------------------|------|
| `langchain_core.tools.tool` | @tool 装饰器 |
| `loop.zvec_store` | 向量查询 |
| `config.env_config` | 会话路径 |

## Tauri 迁移要点

- 工具定义：Rust 中用 `trait Tool { fn name(&self) -> &str; fn invoke(&self, args: Value) -> String; }`
- 工具注册：`Vec<Box<dyn Tool>>` 或 enum + match
- 文件操作：`std::fs` 标准库
- Shell 命令：`std::process::Command`，注意超时和安全性
- 工具列表生成：遍历注册表，拼接描述字符串
- `search_session_memory`：需要异步调用 zvec_store
- `@tool` 装饰器的 docstring → 参数描述：Rust 中用 `serde` + 自定义属性或手动维护描述表
- **安全考虑**：Tauri 应用中应增加路径白名单校验、Shell 命令黑名单等安全措施
