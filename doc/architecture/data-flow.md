# 数据流程分析

## 1. 主流程：用户输入到回答

以用户输入 `"你是谁"` 为例：

```
用户输入: "你是谁"
    │
    ▼
┌─ main.py ─────────────────────────────────────────────────┐
│ user_input = "你是谁"                                     │
│ chat_history = react_loop.react_loop(user_input)          │
│ session_manage.update_current_session(chat_history)       │
│ session_manage.auto_compress_check_from_history(...)      │
└───────────────────────────────────────────────────────────┘
    │
    ▼
┌─ react_loop.py: react_loop("你是谁") ─────────────────────┐
│                                                            │
│ chat_history = [{"role":"user", "content":"你是谁"}]      │
│                                                            │
│ ┌─ 对话层循环 (while True) ──────────────────────────┐    │
│ │                                                     │    │
│ │ chat_result = llm.llm_chat("你是谁", chat_history)  │    │
│ │     │                                               │    │
│ │     ▼                                               │    │
│ │ ┌─ llm.py: llm_chat() ─────────────────────────┐   │    │
│ │ │ model = get_model()  # ChatOpenAI             │   │    │
│ │ │ system_prompt = prompt_assemble               │   │    │
│ │ │                   .assemble_system_prompt()    │   │    │
│ │ │ tool_list = base_tool.get_tool_list()         │   │    │
│ │ │ current_session = session_manage              │   │    │
│ │ │                   .get_session_context_for_   │   │    │
│ │ │                    prompt()                    │   │    │
│ │ │                                                 │   │    │
│ │ │ messages = [SystemMessage, HumanMessage,       │   │    │
│ │ │             AIMessage(chat_history)]           │   │    │
│ │ │ result = model.invoke(messages)                │   │    │
│ │ │ return result                                  │   │    │
│ │ └─────────────────────────────────────────────────┘   │    │
│ │     │                                               │    │
│ │     ▼                                               │    │
│ │ chat_result_content = parse_llm_json(result.content)│    │
│ │                                                     │    │
│ │ # LLM 返回示例:                                     │    │
│ │ # {                                                 │    │
│ │ #   "thought": "用户询问身份，直接回答",             │    │
│ │ #   "message": "我是Avalon，由dudu-feng开发...",    │    │
│ │ #   "next": "stop"                                  │    │
│ │ # }                                                 │    │
│ │                                                     │    │
│ │ print(message)  # 输出给用户                        │    │
│ │ chat_history.append(transform(chat_result))         │    │
│ │                                                     │    │
│ │ next == "stop" → break  # 退出循环                  │    │
│ │ next == "action" → 进入动作层                       │    │
│ └─────────────────────────────────────────────────────┘    │
│                                                            │
│ return chat_history                                        │
└────────────────────────────────────────────────────────────┘
    │
    ▼
┌─ main.py (继续) ──────────────────────────────────────────┐
│ session_manage.update_current_session(chat_history)       │
│   → 将 chat_history 追加到 terminal.json 的 session 数组   │
│                                                            │
│ session_manage.auto_compress_check_from_history(...)      │
│   → 提取 max(input_tokens)，若 >= 阈值则触发压缩           │
└───────────────────────────────────────────────────────────┘
```

## 2. 动作层数据流

当对话层 LLM 返回 `next=action` 时：

```
chat_result = { "next":"action", "action_target":"读取config.json文件" }
    │
    ▼
┌─ 动作层循环 (while True) ────────────────────────────────┐
│                                                           │
│ action_result = llm.llm_action(user_input,               │
│                                 action_target,            │
│                                 action_history)           │
│                                                           │
│ action_result_content = parse_llm_json(result.content)    │
│                                                           │
│ ┌─ 根据 next 分支 ─────────────────────────────────┐     │
│ │                                                   │     │
│ │ next="tool_call":                                 │     │
│ │   tool_name = "read_file"                         │     │
│ │   arguments = {"file_path":"config.json"}         │     │
│ │   tool_result = execute_tool(tool_name, args)     │     │
│ │   action_history.append({tool_call, tool_result}) │     │
│ │   continue → 继续循环                             │     │
│ │                                                   │     │
│ │ next="sub_analysis":                              │     │
│ │   action_history.append({sub_analysis})           │     │
│ │   continue → 继续循环                             │     │
│ │                                                   │     │
│ │ next="finished":                                  │     │
│ │   break → 动作完成                                │     │
│ │                                                   │     │
│ │ 其他: break → 错误终止                             │     │
│ └───────────────────────────────────────────────────┘     │
└───────────────────────────────────────────────────────────┘
    │
    ▼
chat_history.append({"role":"assistant","content":"【执行记录】",
                     "action_history": action_history})
```

## 3. 会话压缩数据流

```
触发条件: auto_compress_check_from_history() 检测到 input_tokens >= 阈值
         或用户手动执行 /compress 命令
    │
    ▼
┌─ session_manage.session_compress() ──────────────────────┐
│                                                           │
│ 1. 读取当前会话 terminal.json                              │
│ 2. current_session["session"] 为空 → 跳过                 │
│                                                           │
│ 3. compressed_data = llm.llm_compress(current_session)   │
│    → LLM 返回: {"summary":[...], "keywords":[...]}        │
│                                                           │
│ 4. parse_llm_json() 解析压缩结果                          │
│ 5. compress_round += 1                                    │
│ 6. compressed.append(compressed_data_content)             │
│                                                           │
│ 7. 原始会话归档:                                          │
│    session_path/history/{session_id}/raw/{chunk}.json     │
│                                                           │
│ 8. 清空 session 数组                                      │
│ 9. 写回 terminal.json                                     │
│                                                           │
│ 10. 同步 ZVec 向量库:                                     │
│     doc_id = "{session_id}_chunk_{N}"                     │
│     zvec_store.insert_session_memory(doc_id, summary,     │
│                                      keywords, timestamp) │
│     → embedding_service.doc_embedding(summary)            │
│     → zvec collection.insert(doc)                         │
│                                                           │
│ 11. _progressive_summarize() 渐进式总结检查               │
└───────────────────────────────────────────────────────────┘
```

## 4. 渐进式总结数据流

```
触发条件: len(compressed) > session_memory_max_chunks (默认10)
    │
    ▼
┌─ _progressive_summarize() ───────────────────────────────┐
│                                                           │
│ merge_batch = max_chunks // 2  (默认5)                    │
│                                                           │
│ ┌─ 构建待合并列表 ─────────────────────────────────┐     │
│ │ 若 super_compressed 已存在:                        │     │
│ │   old_chunks = [super_compressed] + compressed[:4] │     │
│ │   recent_chunks = compressed[4:]                   │     │
│ │ 否则:                                              │     │
│ │   old_chunks = compressed[:5]                      │     │
│ │   recent_chunks = compressed[5:]                   │     │
│ └───────────────────────────────────────────────────┘     │
│                                                           │
│ 收集所有 summary + keywords + chunk范围                   │
│                                                           │
│ 二次压缩: llm.llm_compress(所有summary)                   │
│   → 新的 merged_summary + merged_keywords                 │
│                                                           │
│ 更新会话:                                                  │
│   super_compressed = merged_chunk (永远只1个)              │
│   compressed = recent_chunks (剩余普通块)                  │
│                                                           │
│ ZVec 同步:                                                │
│   删除旧块向量 → 插入超级摘要向量                          │
│                                                           │
│ raw 归档: 保存 merged_{start}_{end}.json                  │
└───────────────────────────────────────────────────────────┘
```

## 5. 会话记忆检索数据流

```
LLM 决策调用 search_session_memory 工具
    │
    ▼
┌─ session_memory_tool.search_session_memory() ─────────────┐
│                                                            │
│ 参数: query="压缩方案", search_mode="hybrid", topk=5      │
│                                                            │
│ 1. 构建 filter_expr (时间范围过滤)                         │
│                                                            │
│ 2. 根据 search_mode 选择查询:                              │
│    ┌─ semantic → zvec_store.vectorQuery_session_memory()  │
│    │             embedding_service.query_embedding(query)  │
│    │             collection.query(vector=queryVector)      │
│    │                                                       │
│    ├─ keyword  → zvec_store.scalarQuery_session_memory()  │
│    │             collection.query(fts=Fts(match_string))   │
│    │                                                       │
│    └─ hybrid   → zvec_store.hybridQuery_session_memory()  │
│                  collection.query(vector + fts)            │
│                                                            │
│ 3. 格式化结果:                                             │
│    每条 → {location, session_id, chunk, description,      │
│            keywords, timestamp, score}                     │
│                                                            │
│ 4. 返回 JSON 字符串给 LLM                                  │
└────────────────────────────────────────────────────────────┘
```

## 6. 数据存储结构

### 6.1 会话文件结构

```
{session_path}/
├── current/
│   └── terminal.json          # 当前活跃会话
└── history/
    └── terminal/
        └── {session_id}/       # 归档会话
            ├── index.json      # 会话元数据
            └── raw/
                ├── 1.json      # 第1次压缩的原始会话
                ├── 2.json      # 第2次压缩的原始会话
                └── merged_1_5.json  # 合并的超级摘要原始数据
```

### 6.2 terminal.json 数据结构

```json
{
  "id": "terminal_2026-06-06-23_03_31",
  "status": "active",
  "compress_round": 3,
  "compressed": [
    {
      "chunk": 1,
      "summary": ["会话总结1", "会话总结2"],
      "keywords": ["关键词1", "关键词2"]
    }
  ],
  "super_compressed": {
    "chunk": "merged_1_5",
    "summary": ["超级摘要1"],
    "keywords": ["超级关键词1"],
    "merged_from_chunks": [1, 2, 3, 4, 5]
  },
  "session": [
    {
      "role": "user",
      "time": "2026-06-06-23:03:31",
      "content": "你是谁"
    },
    {
      "role": "assistant",
      "time": "2026-06-06-23:03:32",
      "content": "我是Avalon...",
      "thought": "用户询问身份",
      "token_usage": {"input_tokens": 500, "output_tokens": 100}
    }
  ]
}
```

### 6.3 ZVec 向量库 Schema

| 字段 | 类型 | 说明 |
|------|------|------|
| `description` | STRING (FTS, jieba分词) | 会话摘要文本 |
| `keyWords` | ARRAY_STRING | 关键词列表 |
| `timestamp` | STRING | 会话时间戳 |
| `summary_vector` | VECTOR_FP32 (512维, COSINE) | 摘要文本向量 |

文档 ID 格式：`{session_id}_chunk_{N}` 或 `{session_id}_chunk_merged_{start}_{end}`
