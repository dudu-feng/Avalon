# Avalon API v1.0 接口文档

> **Base URL**: `http://localhost:8000/api/v1`
>
> **Content-Type**: `application/json; charset=utf-8`
>
> **流式响应**: `text/event-stream` (SSE)

---

## 目录

- [1. 概述](#1-概述)
- [2. 通用约定](#2-通用约定)
- [3. 数据模型](#3-数据模型)
- [4. Chat —— 对话核心](#4-chat--对话核心)
- [5. Session —— 会话管理](#5-session--会话管理)
- [6. Memory —— 会话记忆检索](#6-memory--会话记忆检索)
- [7. Config —— 配置管理](#7-config--配置管理)
- [8. Tool —— 工具列表](#8-tool--工具列表)
- [9. 错误码](#9-错误码)

---

## 1. 概述

Avalon API 为前端提供与 AI 智能体交互的全部能力，核心包括：

- **双层 ReAct 流式对话** — 通过 SSE 实时推送思考、回复、工具调用全过程
- **多会话管理** — 创建、切换、压缩、归档历史会话
- **永恒会话记忆** — 跨会话的语义/关键词/混合检索
- **配置管理** — LLM 参数、压缩阈值等运行时可调

### 1.1 核心流程

```
POST /chat/send (SSE)                 POST /sessions/{id}/compress
      │                                       │
      ▼                                       ▼
前端发送消息 ──→ 后端 ReAct 循环 ──→ 流式推回每一步
                      │
              ┌───────┴───────┐
              ▼               ▼
        对话层 LLM        动作层 LLM
        (思考+回复)       (工具调用+子分析)
                              │
                              ▼
                        execute_tool()
```

### 1.2 SSE 事件时序

以一次典型对话为例（用户问"帮我读一下 config.json"）：

```
chat_start → chat_thought → chat_message → chat_stop → done
                                                          
# 如果 LLM 决定执行动作:
chat_start → action_start → action_step → action_tool_result
          → action_step → action_finished → done
```

---

## 2. 通用约定

### 2.1 请求头

```
Content-Type: application/json
Accept: application/json            # 普通请求
Accept: text/event-stream           # SSE 流式请求
```

### 2.2 响应格式

所有非流式接口统一返回：

```json
{
  "code": 0,
  "message": "success",
  "data": { ... }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `code` | `int` | 业务状态码，`0` 表示成功 |
| `message` | `string` | 状态描述 |
| `data` | `object` / `array` / `null` | 业务数据 |

### 2.3 时间格式

所有时间字段统一使用 `YYYY-MM-DD-HH:MM:SS` 格式（与现有终端版兼容）。

### 2.4 会话 ID 格式

```
terminal_YYYY-MM-DD-HH_MM_SS
```

示例: `terminal_2026-08-07-20_30_00`

---

## 3. 数据模型

### 3.1 会话对象 (Session)

```json
{
  "id": "terminal_2026-08-07-20_30_00",
  "status": "active",
  "compress_round": 3,
  "compressed": [
    {
      "chunk": 1,
      "summary": ["用户询问了项目结构", "Avalon 介绍了目录布局"],
      "keywords": ["项目结构", "目录", "Avalon"]
    }
  ],
  "super_compressed": {
    "chunk": "merged_1_5",
    "summary": ["这是一个涵盖项目初始化到配置完成的超级摘要"],
    "keywords": ["项目初始化", "配置"],
    "merged_from_chunks": [1, 2, 3, 4, 5]
  },
  "session": [
    {
      "role": "user",
      "time": "2026-08-07-20:30:05",
      "content": "你是谁"
    },
    {
      "role": "assistant",
      "time": "2026-08-07-20:30:06",
      "content": "我是 Avalon，由 dudu-feng 开发的智能体",
      "thought": "用户询问身份，直接回答",
      "token_usage": {
        "input_tokens": 850,
        "output_tokens": 120,
        "total_tokens": 970
      }
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | `string` | 唯一标识 |
| `status` | `string` | `active` / `archived` / `inactive` |
| `compress_round` | `int` | 已执行压缩轮数 |
| `compressed` | `array<Chunk>` | 普通压缩块列表 |
| `super_compressed` | `Chunk\|null` | 超级摘要（合并后的历史摘要，最多 1 个） |
| `session` | `array<Message>` | 当前未压缩的活跃消息 |

### 3.2 压缩块对象 (Chunk)

```json
{
  "chunk": 1,
  "summary": ["摘要文本1", "摘要文本2"],
  "keywords": ["关键词1", "关键词2"],
  "merged_from_chunks": [1, 2, 3]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `chunk` | `int \| string` | 普通块为 int，合并块为 `"merged_1_5"` |
| `summary` | `array<string>` | LLM 生成的摘要列表 |
| `keywords` | `array<string>` | LLM 提取的关键词 |
| `merged_from_chunks` | `array<int>` | 仅合并块有此字段，记录来源块编号 |

### 3.3 消息对象 (Message)

**用户消息**:
```json
{
  "role": "user",
  "time": "2026-08-07-20:30:05",
  "content": "帮我读一下 config.json"
}
```

**助手消息（对话层）**:
```json
{
  "role": "assistant",
  "time": "2026-08-07-20:30:06",
  "content": "好的，我来读取 config.json 的内容",
  "thought": "用户想要读取配置文件，我需要执行 read_file 工具",
  "token_usage": {
    "input_tokens": 850,
    "output_tokens": 80,
    "total_tokens": 930
  }
}
```

**助手消息（含动作记录）**:
```json
{
  "role": "assistant",
  "content": "【执行记录】",
  "action_history": [
    {
      "action_target": "读取 config.json 文件"
    },
    {
      "step": "tool_call",
      "time": "2026-08-07-20:30:07",
      "analysis": "需要先读取文件内容",
      "action": {
        "name": "read_file",
        "arguments": { "file_path": "/path/to/config.json" }
      },
      "tool_result": "{ \"key\": \"value\" }",
      "token_usage": { "input_tokens": 400, "output_tokens": 60 }
    },
    {
      "step": "finished",
      "time": "2026-08-07-20:30:08",
      "analysis": "文件读取成功，将内容返回给用户"
    }
  ]
}
```

### 3.4 会话记忆检索结果 (MemoryHit)

```json
{
  "location": "/data/session/history/terminal/terminal_2026-07-01-.../raw/2.json",
  "session_id": "terminal_2026-07-01-10_00_00",
  "chunk": 2,
  "description": "用户询问了关于项目压缩方案的问题...",
  "keywords": ["压缩", "token", "阈值"],
  "timestamp": "2026-07-01-10_00_00",
  "score": 0.8732
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `location` | `string` | 原始对话文件路径 |
| `session_id` | `string` | 所属会话 ID |
| `chunk` | `int \| string` | 压缩块序号 |
| `description` | `string` | 压缩摘要文本 |
| `keywords` | `array<string>` | 关键词 |
| `timestamp` | `string` | 会话发生时间 |
| `score` | `float` | 相关度分数（0~1，越高越相关） |

---

## 4. Chat —— 对话核心

### 4.1 发送消息 (SSE 流式)

```
POST /chat/send
```

发起一次对话。后端执行双层 ReAct 循环，通过 **Server-Sent Events** 实时推送每一步进展。

#### 请求体

```json
{
  "session_id": "terminal_2026-08-07-20_30_00",
  "message": "帮我查一下我们之前讨论过的压缩方案"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | `string` | ✅ | 目标会话 ID |
| `message` | `string` | ✅ | 用户输入（1 ~ 100000 字符） |

#### SSE 事件规范

**连接**：客户端发起 POST 后，服务端返回 `Content-Type: text/event-stream`，保持长连接直到 `done` 事件。

**通用字段约定**：

| 字段 | 说明 |
|------|------|
| `event` | 事件类型（见下表） |
| `data` | JSON 字符串，包含事件负载 |
| `id` | 事件序号（递增整数） |

---

#### 事件类型一览

| 事件 | 方向 | 说明 | 是否终止流 |
|------|------|------|:---:|
| `chat_start` | S→C | 对话层 LLM 开始推理 | |
| `chat_thought` | S→C | LLM 思考过程 | |
| `chat_message` | S→C | 回复内容（若流式则多次推送 delta） | |
| `chat_stop` | S→C | 对话结束（LLM 决定无需执行动作） | |
| `action_start` | S→C | 进入动作层，开始执行目标 | |
| `action_step` | S→C | 动作层每一步的判断 | |
| `action_tool_call` | S→C | 即将调用工具 | |
| `action_tool_result` | S→C | 工具执行结果返回 | |
| `action_sub_analysis` | S→C | 子步骤分析/规划 | |
| `action_finished` | S→C | 动作层完成 | |
| `error` | S→C | 发生错误 | ✅ |
| `done` | S→C | 整个 ReAct 循环结束 | ✅ |

---

#### 事件详细定义

##### `chat_start`

对话层开始。此时前端可显示 "思考中..." 动画。

```
event: chat_start
data: {"message_id": "msg_abc123"}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `message_id` | `string` | 本条消息唯一 ID |

##### `chat_thought`

LLM 的思考内容。前端可折叠显示或灰色小字展示。

```
event: chat_thought
data: {"content": "用户想查看之前讨论过的压缩方案，我需要先搜索会话记忆"}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `content` | `string` | 思考文本 |

##### `chat_message`

回复内容。如果 LLM 返回完整 JSON，则一次性推送；如果升级到流式 `model.stream()`，则多次推送 `delta`。

```
event: chat_message
data: {"delta": "好的，让我来搜索一下我们之前关于压缩方案的讨论。"}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `delta` | `string` | 增量文本（非流式时为完整 message） |

##### `chat_stop`

LLM 判定无需进一步动作，对话层直接结束。

```
event: chat_stop
data: {"token_usage": {"input_tokens": 850, "output_tokens": 120, "total_tokens": 970}}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `token_usage` | `object` | 本次 LLM 调用的 token 消耗 |

##### `action_start`

进入动作层。前端可切换 UI 状态（如展开操作面板）。

```
event: action_start
data: {"action_target": "搜索历史会话中关于压缩方案的记录"}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `action_target` | `string` | 动作目标描述 |

##### `action_step`

动作层 LLM 每一步的决策分析。

```
event: action_step
data: {
  "analysis": "需要调用 search_session_memory 工具来检索历史记录",
  "next": "tool_call"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `analysis` | `string` | 当前步骤分析 |
| `next` | `string` | 下一步类型：`tool_call` / `sub_analysis` / `finished` |

##### `action_tool_call`

即将执行工具调用。

```
event: action_tool_call
data: {
  "tool_name": "search_session_memory",
  "arguments": {
    "query": "压缩方案",
    "search_mode": "hybrid",
    "topk": 5
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `tool_name` | `string` | 工具名称 |
| `arguments` | `object` | 工具参数 |

##### `action_tool_result`

工具执行完成。

```
event: action_tool_result
data: {
  "tool_name": "search_session_memory",
  "success": true,
  "result": "[{\"description\": \"压缩相关...\", \"score\": 0.92}]"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `tool_name` | `string` | 工具名称 |
| `success` | `bool` | 执行是否成功 |
| `result` | `string` | 工具返回的原始字符串 |

##### `action_sub_analysis`

动作层进行子步骤分析/规划（不调用工具，纯推理）。

```
event: action_sub_analysis
data: {
  "analysis": "需要进一步分析搜索结果",
  "sub_analysis": "找到 3 条相关记忆，其中 7月3日 的讨论最为详细..."
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `analysis` | `string` | 当前步骤分析 |
| `sub_analysis` | `string` | 子分析内容 |

##### `action_finished`

动作层执行完成，即将回到对话层继续。

```
event: action_finished
data: {
  "analysis": "已成功搜索到相关历史记录，可以汇总回复用户",
  "token_usage": {"input_tokens": 600, "output_tokens": 150, "total_tokens": 750}
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `analysis` | `string` | 完成分析 |
| `token_usage` | `object` | 动作层 LLM 累计 token 消耗 |

##### `error`

任何环节发生错误，流立即终止。

```
event: error
data: {
  "code": 50001,
  "message": "LLM 调用超时",
  "detail": "ChatOpenAI timeout after 60s"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `code` | `int` | 错误码（见 [错误码](#9-错误码)） |
| `message` | `string` | 用户可读的错误描述 |
| `detail` | `string` | 技术细节 |

##### `done`

整个 ReAct 循环正常结束，携带完整的本轮聊天历史。

```
event: done
data: {
  "chat_history": [
    {"role": "user", "time": "...", "content": "..."},
    {"role": "assistant", "time": "...", "content": "...", "thought": "..."},
    ...
  ],
  "compress_triggered": false
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `chat_history` | `array<Message>` | 本轮产生的完整消息列表 |
| `compress_triggered` | `bool` | 是否在本轮触发了自动压缩 |

---

#### SSE 客户端示例

```javascript
async function sendMessage(sessionId, message) {
  const response = await fetch('http://localhost:8000/api/v1/chat/send', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Accept': 'text/event-stream',
    },
    body: JSON.stringify({ session_id: sessionId, message }),
  });

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';   // 保留不完整的行

    for (const line of lines) {
      if (line.startsWith('event: ')) {
        currentEvent = line.slice(7);
      } else if (line.startsWith('data: ')) {
        const data = JSON.parse(line.slice(6));
        handleEvent(currentEvent, data);
      }
    }
  }
}

function handleEvent(event, data) {
  switch (event) {
    case 'chat_thought':
      showThought(data.content);    break;
    case 'chat_message':
      appendMessage(data.delta);    break;
    case 'action_tool_call':
      showToolCall(data.tool_name); break;
    case 'action_tool_result':
      showToolResult(data.result);  break;
    case 'error':
      showError(data.message);      break;
    case 'done':
      finalize(data.chat_history);  break;
  }
}
```

---

## 5. Session —— 会话管理

### 5.1 获取会话列表

```
GET /sessions
```

返回所有会话（当前活跃 + 历史归档）。

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "current": {
      "id": "terminal_2026-08-07-20_30_00",
      "status": "active",
      "compress_round": 3,
      "message_count": 15,
      "last_message_time": "2026-08-07-20:45:12"
    },
    "history": [
      {
        "id": "terminal_2026-07-15-14_20_00",
        "status": "archived",
        "compress_round": 5,
        "message_count": 42,
        "last_message_time": "2026-07-15-16:30:00"
      }
    ]
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `current` | `object\|null` | 当前活跃会话，无则为 null |
| `current.id` | `string` | 会话 ID |
| `current.status` | `string` | 固定为 `"active"` |
| `current.compress_round` | `int` | 已压缩轮数 |
| `current.message_count` | `int` | 未压缩消息数量 |
| `current.last_message_time` | `string` | 最后一条消息时间 |
| `history` | `array` | 历史归档会话列表，按时间倒序 |

> **实现提示**：`message_count` = `len(session["session"])`，`last_message_time` = 最后一条 session 消息的 `time` 字段。历史列表从 `session_path/history/terminal/` 目录遍历 `index.json` 汇总。

---

### 5.2 创建新会话

```
POST /sessions
```

#### 请求体

```json
{
  "preserve_current": true
}
```

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `preserve_current` | `bool` | | `true` | 是否先归档当前会话再创建新的 |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "id": "terminal_2026-08-07-21_00_00",
    "status": "active",
    "created_at": "2026-08-07-21:00:00"
  }
}
```

#### 逻辑

1. 若 `preserve_current=true` 且当前存在活跃会话，先调用 `save_current_session()` 归档
2. 生成新 ID `terminal_{当前时间戳}`
3. 创建 `current/terminal.json`，初始化空结构
4. 返回新会话 ID

---

### 5.3 获取会话详情

```
GET /sessions/{session_id}
```

#### 路径参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `session_id` | `string` | 会话 ID |

#### 查询参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `include_raw` | `bool` | | `false` | 是否包含压缩块的原始对话 |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "id": "terminal_2026-08-07-20_30_00",
    "status": "active",
    "compress_round": 3,
    "compressed": [ ... ],
    "super_compressed": { ... },
    "session": [ ... ]
  }
}
```

`data` 为完整的 [会话对象 (Session)](#31-会话对象-session)。

#### 特殊情况

- **活跃会话**：读取 `session_path/current/terminal.json`
- **归档会话**：读取 `session_path/history/terminal/{session_id}/index.json`
- **会话不存在**：返回 `404`

---

### 5.4 压缩会话

```
POST /sessions/{session_id}/compress
```

触发 LLM 压缩 + ZVec 同步 + 渐进式总结检查。

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "session_id": "terminal_2026-08-07-20_30_00",
    "compress_round": 4,
    "archived_messages": 12,
    "chunk_summary": ["本次对话讨论了项目配置", "用户询问了 API 设计"],
    "chunk_keywords": ["配置", "API设计"],
    "progressive_merged": false
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `session_id` | `string` | 会话 ID |
| `compress_round` | `int` | 压缩后轮数 |
| `archived_messages` | `int` | 本次归档的消息数 |
| `chunk_summary` | `array<string>` | LLM 生成的压缩摘要 |
| `chunk_keywords` | `array<string>` | LLM 提取的关键词 |
| `progressive_merged` | `bool` | 是否触发了渐进式总结（旧块合并） |

#### 特殊情况

- **会话为空**（`session` 数组长度为 0）：返回 `code=40001`，message=当前会话无消息可压缩

---

### 5.5 归档会话

```
POST /sessions/{session_id}/archive
```

将当前活跃会话归档到历史目录，重置 current 为空。

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "session_id": "terminal_2026-08-07-20_30_00",
    "archived_at": "2026-08-07-21:30:00"
  }
}
```

> **实现提示**：此接口内部会先调用 `session_compress()` 压缩剩余消息，再将 status 改为 `archived`，移动到 history 目录。压缩失败不影响归档（跳过压缩直接归档原始数据）。

---

### 5.6 删除会话

```
DELETE /sessions/{session_id}
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "session_id": "terminal_2026-07-01-10_00_00",
    "deleted_files": 5,
    "zvec_entries_removed": 3
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `deleted_files` | `int` | 删除的 raw 归档文件数 |
| `zvec_entries_removed` | `int` | 从向量库移除的条目数 |

#### 特殊情况

- **活跃会话不可删除**：返回 `code=40002`，需先归档再删除
- 删除操作需同时清理：`history/{session_id}/` 目录 + ZVec 中 `{session_id}_chunk_*` 条目

---

## 6. Memory —— 会话记忆检索

### 6.1 搜索历史记忆

```
POST /memory/search
```

在压缩后的会话摘要中检索与查询最相关的内容。支持三种检索模式。

#### 请求体

```json
{
  "query": "我们之前讨论过的压缩方案",
  "search_mode": "hybrid",
  "topk": 5,
  "time_range": "2026-07-01,2026-08-01"
}
```

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `query` | `string` | ✅ | | 搜索文本，支持自然语言和关键词 |
| `search_mode` | `string` | | `"hybrid"` | `semantic` / `keyword` / `hybrid` |
| `topk` | `int` | | `5` | 返回结果数（1~20） |
| `time_range` | `string` | | `""` | 时间过滤，见下文格式说明 |

#### time_range 格式

| 格式 | 示例 | 含义 |
|------|------|------|
| 空字符串 | `""` | 不限时间 |
| 单日期 | `"2026-07-15"` | 该日期及之后 |
| 日期区间 | `"2026-07-01,2026-08-01"` | 闭区间内 |

#### search_mode 说明

| 模式 | 检索方式 | 适用场景 |
|------|---------|---------|
| `semantic` | 向量语义相似度 (COSINE) | 模糊回忆、同义改写（"退出"→"结束对话"） |
| `keyword` | FTS 全文检索 (jieba 分词) | 精确关键词查找 |
| `hybrid` | 语义 + 关键词混合，综合排序 | 通用场景（推荐） |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "results": [
      {
        "location": "/data/session/history/terminal/terminal_2026-07-03-15_20_00/raw/2.json",
        "session_id": "terminal_2026-07-03-15_20_00",
        "chunk": 2,
        "description": "用户询问了 LLM 上下文长度的压缩策略，讨论了 token 阈值设置和渐进式总结方案",
        "keywords": ["压缩", "token", "阈值", "渐进式总结"],
        "timestamp": "2026-07-03-15_20_00",
        "score": 0.9215
      }
    ],
    "total": 3
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `results` | `array<MemoryHit>` | [检索结果列表](#34-会话记忆检索结果-memoryhit) |
| `total` | `int` | 命中总数 |

#### 空结果

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "results": [],
    "total": 0
  }
}
```

---

### 6.2 获取压缩块原始对话

```
GET /sessions/{session_id}/raw/{chunk}
```

当用户从检索结果中点击某个记忆片段时，加载对应的完整原始对话。

#### 路径参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `session_id` | `string` | 会话 ID |
| `chunk` | `string` | 块标识（`1`, `2`, `merged_1_5` 等） |

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "session_id": "terminal_2026-07-03-15_20_00",
    "chunk": 2,
    "type": "compressed_chunk",
    "messages": [
      {"role": "user", "time": "...", "content": "..."},
      {"role": "assistant", "time": "...", "content": "...", "thought": "..."}
    ]
  }
}
```

> **实现提示**：读取 `session_path/history/terminal/{session_id}/raw/{chunk}.json`。合并块（`merged_*`）的 type 为 `"merged_summary"`。

---

## 7. Config —— 配置管理

### 7.1 获取配置

```
GET /config
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "model": {
      "name": "deepseek-v4-pro",
      "base_url": "https://api.example.com/v1",
      "api_key": "sk-****a1b2"
    },
    "session": {
      "compress_threshold": 20000,
      "max_chunks": 10,
      "context_chunks": 5
    },
    "embedding": {
      "mode": "local",
      "model": "bge-small-zh-v1.5",
      "device": "cpu"
    }
  }
}
```

> **安全约定**：`api_key` 字段始终脱敏：保留前 3 位 + `****` + 后 4 位。

### 7.2 更新配置

```
PUT /config
```

#### 请求体（全部可选，仅传要更新的字段）

```json
{
  "model": {
    "name": "deepseek-v4-pro",
    "base_url": "https://api.example.com/v1",
    "api_key": "sk-new-key"
  },
  "session": {
    "compress_threshold": 32000,
    "max_chunks": 15,
    "context_chunks": 8
  }
}
```

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "updated": ["model.api_key", "session.compress_threshold", "session.max_chunks", "session.context_chunks"],
    "needs_restart": false
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `updated` | `array<string>` | 实际更新的字段路径 |
| `needs_restart` | `bool` | 是否需要重启服务生效 |

> **实现提示**：写入 `.env` 文件，`env_config` 单例需提供 `reload()` 方法以支持热更新。`needs_restart=true` 的字段（如 `embedding_mode`、`embedding_device`）需标记。

---

## 8. Tool —— 工具列表

### 8.1 获取可用工具

```
GET /tools
```

用于前端展示当前智能体具备的能力。

#### 响应

```json
{
  "code": 0,
  "message": "success",
  "data": {
    "tools": [
      {
        "name": "read_file",
        "description": "读取指定文件的内容，传入参数 file_path 为文件路径",
        "parameters": {
          "file_path": { "type": "string", "required": true, "description": "文件路径" }
        }
      },
      {
        "name": "write_file",
        "description": "创建或覆盖写入文件",
        "parameters": {
          "file_path": { "type": "string", "required": true, "description": "文件路径" },
          "content": { "type": "string", "required": true, "description": "文件内容" }
        }
      },
      {
        "name": "delete_file",
        "description": "删除指定文件",
        "parameters": {
          "file_path": { "type": "string", "required": true, "description": "文件路径" }
        }
      },
      {
        "name": "run_shell_command",
        "description": "在终端执行命令，并返回标准输出和错误",
        "parameters": {
          "command": { "type": "string", "required": true, "description": "Shell 命令字符串" }
        }
      },
      {
        "name": "get_directory_contents",
        "description": "获取指定目录下的所有文件和子目录",
        "parameters": {
          "directory_path": { "type": "string", "required": true, "description": "目录路径" }
        }
      },
      {
        "name": "search_session_memory",
        "description": "搜索历史会话记忆。在压缩后的会话摘要中检索相关内容",
        "parameters": {
          "query": { "type": "string", "required": true, "description": "搜索文本" },
          "search_mode": { "type": "string", "required": false, "description": "semantic / keyword / hybrid" },
          "topk": { "type": "int", "required": false, "description": "返回数量，默认5" },
          "time_range": { "type": "string", "required": false, "description": "时间过滤" }
        }
      }
    ]
  }
}
```

> **实现提示**：从 `base_tool.TOOLS` 列表遍历，通过 LangChain 的 `tool.args_schema` 提取参数定义。

---

## 9. 错误码

### 9.1 HTTP 状态码

| 状态码 | 含义 |
|:------:|------|
| `200` | 成功 |
| `400` | 请求参数错误 |
| `404` | 资源不存在 |
| `500` | 服务端内部错误 |

### 9.2 业务错误码

| 错误码 | 说明 | 来源接口 |
|:------:|------|----------|
| `0` | 成功 | 全部 |
| `40001` | 当前会话无消息可压缩 | `POST /sessions/{id}/compress` |
| `40002` | 活跃会话不可删除，请先归档 | `DELETE /sessions/{id}` |
| `40003` | 会话不存在 | `GET/POST/DELETE /sessions/{id}` |
| `40004` | 请求参数校验失败 | 全部 |
| `50001` | LLM 调用失败或超时 | `POST /chat/send` |
| `50002` | LLM 返回的 JSON 无法解析 | `POST /chat/send` |
| `50003` | 工具执行异常 | `POST /chat/send`（流内 error 事件） |
| `50004` | 压缩模型调用失败 | `POST /sessions/{id}/compress` |
| `50005` | 向量数据库操作失败 | `POST /memory/search` |
| `50006` | 文件读写失败 | 涉及文件操作的接口 |
| `50007` | 配置写入失败 | `PUT /config` |

---

## 附录 A：前端状态机参考

聊天 UI 可根据 SSE 事件驱动状态切换：

```
IDLE ──(用户发送)──→ WAITING ──(chat_start)──→ THINKING
                                                    │
                                          ┌─────────┴──────────┐
                                     (chat_message)      (action_start)
                                          │                    │
                                          ▼                    ▼
                                      REPLYING            ACTING
                                          │                    │
                                     (chat_stop)    (action_finished)
                                          │                    │
                                          └─────────┬──────────┘
                                                    ▼
                                                  IDLE
                                              (done 事件)
```

---

## 附录 B：与终端版 main.py 的对应关系

| 终端版 main.py | API 接口 |
|----------------|----------|
| `main.py:4` — `init_session()` | `GET /sessions` 查看 → 前端选择 → `POST /sessions` |
| `main.py:15` — `input("You > ")` | `POST /chat/send` 的 `message` 字段 |
| `main.py:25` — `/compress` 命令 | `POST /sessions/{id}/compress` |
| `main.py:28` — `/exit` 命令 | `POST /sessions/{id}/archive` |
| `main.py:37` — `react_loop(user_input)` | `POST /chat/send` SSE 流（后端内部调用） |
| `main.py:38` — `update_current_session()` | `POST /chat/send` 的 `done` 事件后后端自动执行 |
| `main.py:40` — `auto_compress_check_from_history()` | `POST /chat/send` 的 `done` 事件后后端自动执行 |
| `main.py:43` — `save_current_session()` | `POST /sessions/{id}/archive` |
| 无终端对应 | `POST /memory/search` — 纯前端驱动的记忆检索 |
| 无终端对应 | `GET/PUT /config` — 前端设置页面 |
