# 会话管理模块 (`loop/session_manage.py`)

## 概述

会话管理模块是 Avalon **永恒会话机制** 的核心实现。负责会话的创建、更新、压缩、归档，以及渐进式总结和上下文裁剪。通过 LLM 压缩 + 向量检索实现无限长度的会话记忆。

## 文件清单

| 文件 | 说明 |
|------|------|
| `loop/session_manage.py` | 会话管理全部逻辑 |

## 数据模型

### terminal.json 结构

```json
{
  "id": "terminal_2026-06-06-23_03_31",   // 会话唯一ID（含时间戳）
  "status": "active",                       // active / archived / inactive
  "compress_round": 3,                      // 压缩轮次计数
  "compressed": [                           // 普通压缩块数组
    {
      "chunk": 1,                           // 块序号（int）
      "summary": ["摘要1", "摘要2"],         // LLM 压缩摘要
      "keywords": ["关键词1", "关键词2"]     // LLM 提取关键词
    }
  ],
  "super_compressed": {                     // 超级摘要（永远最多1个）
    "chunk": "merged_1_5",                  // 合并范围标识
    "summary": ["超级摘要1"],
    "keywords": ["超级关键词1"],
    "merged_from_chunks": [1, 2, 3, 4, 5]   // 合并来源
  },
  "session": [                              // 当前活跃会话（未压缩）
    {"role": "user", "content": "...", "time": "..."},
    {"role": "assistant", "content": "...", "thought": "...", "token_usage": {...}}
  ]
}
```

## 核心函数

### 1. `init_session()`

**功能**：初始化当前会话。程序启动时调用。

**实现逻辑**：
1. 读取 `session_path/current/terminal.json`
2. 若文件不存在 → 创建新会话结构
3. 若 `status == "active"` → 提示"继续上次对话"
4. 否则 → 创建新会话（生成时间戳 ID，初始化空结构）

**调用方**：`main.py` 启动时

---

### 2. `get_current_session() -> dict`

**功能**：读取当前会话完整数据。

**返回**：terminal.json 的完整 dict。

**调用方**：`session_compress()`, `get_session_context_for_prompt()`, `_progressive_summarize()`

---

### 3. `update_current_session(chat_history: list)`

**功能**：将新一轮聊天历史追加到当前会话。

**实现逻辑**：
1. 读取 terminal.json
2. `data["session"].extend(chat_history)`
3. 写回文件

**调用方**：`main.py` 每轮对话后

---

### 4. `save_current_session()`

**功能**：退出前归档当前会话。

**实现逻辑**：
1. 调用 `session_compress()` 压缩剩余会话
2. 读取当前会话，设 `status = "archived"`
3. 创建归档目录 `session_path/history/terminal/{session_id}/`
4. 写入 `index.json`（完整会话元数据）
5. 重置 current/terminal.json 为空 inactive 状态

**调用方**：`main.py` 退出时

---

### 5. `session_compress()`

**功能**：压缩当前会话，是永恒会话机制的核心。

**实现逻辑**：
1. 读取当前会话，若 `session` 为空 → 跳过
2. 调用 `llm.llm_compress(current_session)` → LLM 压缩
3. `parse_llm_json()` 解析结果，失败则中止
4. `compress_round += 1`
5. 追加压缩块到 `compressed` 数组
6. **原始数据归档**：写入 `history/{session_id}/raw/{chunk}.json`
7. **清空 session 数组**
8. 写回 terminal.json
9. **ZVec 同步**：`zvec_store.insert_session_memory(doc_id, summary, keywords, timestamp)`
10. **渐进式总结检查**：`_progressive_summarize()`

**doc_id 格式**：`{session_id}_chunk_{N}`

**调用方**：`main.py`（/compress 命令）、`auto_compress_check_from_history()`、`save_current_session()`

---

### 6. `auto_compress_check_from_history(chat_history: list) -> bool`

**功能**：从聊天历史中提取最大 input_tokens，判断是否触发自动压缩。

**实现逻辑**：
1. 递归遍历 `chat_history` 及嵌套的 `action_history`
2. 收集所有 `token_usage.input_tokens`
3. 取最大值 `max_input`
4. 若 `max_input >= env_config.session_memory_compress_threshold` → 触发 `session_compress()`

**返回**：`True` 已触发，`False` 未达到阈值。

**调用方**：`main.py` 每轮对话后

---

### 7. `get_session_context_for_prompt() -> dict`

**功能**：返回限界会话上下文，用于 LLM 系统提示。

**裁剪策略**：
- `super_compressed`：**始终包含**（代表完整历史）
- `compressed`：只包含最近 N 个（`session_memory_context_chunks`，默认 5）
- 旧块：不加载，由 `search_session_memory` 工具按需检索
- 返回浅拷贝，不影响文件中的完整数据

**当 compressed 数量 <= max_context**：直接返回完整数据。
**当 compressed 数量 > max_context**：裁剪并添加 `_older_chunks_omitted` 字段标记省略数量。

**调用方**：`llm.llm_chat()`

---

### 8. `_progressive_summarize()`

**功能**：永恒会话核心——渐进式总结，防止 compressed 数组无限膨胀。

**触发条件**：`len(compressed) > session_memory_max_chunks`（默认 10）

**合并策略**：
```
merge_batch = max_chunks // 2  (默认 5)

若 super_compressed 已存在:
  old_chunks = [super_compressed] + compressed[:merge_batch-1]
  recent_chunks = compressed[merge_batch-1:]
否则:
  old_chunks = compressed[:merge_batch]
  recent_chunks = compressed[merge_batch:]
```

**实现逻辑**：
1. 收集 old_chunks 的所有 summary、keywords、chunk 编号范围
2. 计算合并范围 `merged_{start}_{end}`
3. 二次压缩：`llm.llm_compress(所有summary)` → 新的超级摘要
4. 压缩失败时降级：简单拼接前 3 条摘要
5. 更新会话：
   - `super_compressed = merged_chunk`（永远只 1 个）
   - `compressed = recent_chunks`（剩余普通块）
6. **ZVec 同步**：删除旧块向量 → 插入超级摘要向量
7. **raw 归档**：保存 `merged_{start}_{end}.json`

**膨胀抑制示例**（max_chunks=10, merge_batch=5）：
```
第1次: [1,2,3,4,5]                     → super=merged_1_5,    compressed=[6,7,8,9,10]
第2次: super + [6,7,8,9]              → super=merged_1_9,    compressed=[10,11,12,13]
第3次: super + [10,11,12,13]          → super=merged_1_13,   compressed=[14,15,16,17]
...永不膨胀
```

**调用方**：`session_compress()` 末尾

## 依赖关系

| 依赖 | 用途 |
|------|------|
| `config.env_config` | 路径、阈值配置 |
| `llm.llm` | 压缩模型调用 |
| `loop.react_loop` | JSON 解析 |
| `loop.zvec_store` | 向量记忆写入/删除 |

## Tauri 迁移要点

- 会话文件用 `serde_json` 序列化/反序列化，定义 `struct Session` 强类型
- `session_memory_compress_threshold` 解析（`"64k"` → 64000）需在 Rust 实现
- 递归 token 统计用 Rust 递归遍历
- 渐进式总结的块编号解析逻辑需移植（`_extract_nums` 等辅助函数）
- 文件 I/O 用 `std::fs`，需处理并发写入安全
- 归档目录结构保持一致，便于 Python/Rust 互操作
