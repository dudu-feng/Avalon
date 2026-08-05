# LLM 交互模块 (`llm/`)

## 概述

封装所有与 LLM 的交互，提供三种调用模式：**对话 (chat)**、**动作执行 (action)**、**会话压缩 (compress)**。使用 LangChain 的 `ChatOpenAI` 作为底层客户端，兼容 OpenAI API 格式。

## 文件清单

| 文件 | 说明 |
|------|------|
| `llm/__init__.py` | 导出 `llm` 子模块 |
| `llm/llm.py` | LLM 调用封装（三种模式） |

## 核心函数

### 1. `get_model() -> ChatOpenAI`

**功能**：创建标准 ChatOpenAI 客户端实例。

**实现逻辑**：
```python
def get_model() -> ChatOpenAI:
    return ChatOpenAI(
        api_key=env_config.default_api_key,
        model_name=env_config.default_model,
        base_url=env_config.default_model_base_url,
    )
```

**调用方**：`llm_chat()`

---

### 2. `get_jsonOutput_model() -> ChatOpenAI`

**功能**：创建强制 JSON 输出的 ChatOpenAI 客户端。

**实现逻辑**：在 `get_model()` 基础上添加 `model_kwargs.response_format = {"type": "json_object"}`，强制模型返回合法 JSON。

**调用方**：`llm_action()`, `llm_compress()`

---

### 3. `llm_chat(user_input: str, chat_history: list) -> BaseMessage`

**功能**：对话层 LLM 调用。接收用户输入和聊天历史，返回模型响应。

**实现逻辑**：
1. 获取标准模型 `get_model()`
2. 组装系统提示：
   - `prompt_assemble.assemble_system_prompt()` — 基础设定 + Markdown 提示词
   - `response_template` — JSON 输出格式要求
   - `base_tool.get_tool_list()` — 可用工具列表
   - `session_manage.get_session_context_for_prompt()` — 历史会话上下文
3. 构造消息列表：`[SystemMessage, HumanMessage(user_input), AIMessage(chat_history)]`
4. 调用 `model.invoke(messages)` 返回结果

**预期 LLM 输出格式**（JSON）：
```json
{
  "thought": "分析当前情况",
  "message": "输出给用户的消息",
  "next": "action 或 stop",
  "action_target": "当 next=action 时，目标描述"
}
```

**调用方**：`react_loop.react_loop()`

---

### 4. `llm_action(user_input: str, action_target: str, action_history: list) -> BaseMessage`

**功能**：动作层 LLM 调用。执行分步骤任务，支持工具调用。

**实现逻辑**：
1. 获取 JSON 输出模型 `get_jsonOutput_model()`
2. 组装系统提示：
   - action 目标描述
   - JSON 输出格式要求（含 `tool_call`/`sub_analysis`/`finished` 三种 next）
   - `action_history` — 已执行的操作历史（防止死循环）
   - `base_tool.get_tool_list()` — 工具列表
3. 消息列表：`[SystemMessage, HumanMessage(user_input)]`
4. 调用 `model.invoke(messages)`

**预期 LLM 输出格式**（JSON）：
```json
{
  "analysis": "分析当前情况",
  "next": "tool_call / sub_analysis / finished",
  "tool_call": {
    "name": "工具名称",
    "arguments": {}
  },
  "sub_analysis": "子步骤分析（仅 next=sub_analysis 时）"
}
```

**调用方**：`react_loop.react_loop()` 动作层循环

---

### 5. `llm_compress(session_data: dict) -> BaseMessage`

**功能**：会话压缩 LLM 调用。将历史会话压缩为摘要和关键词。

**实现逻辑**：
1. 获取 JSON 输出模型
2. 系统提示要求返回 `{"summary": [...], "keywords": [...]}`
3. 用户提示传入 `session_data`（完整会话数据）
4. 调用 `model.invoke(messages)`

**预期 LLM 输出格式**（JSON）：
```json
{
  "summary": ["会话总结1", "会话总结2"],
  "keywords": ["关键词1", "关键词2"]
}
```

- `summary`：单个总结不超过 200 字符（后续需向量化）
- `keywords`：精炼关键词，用于关键词检索

**调用方**：`session_manage.session_compress()`, `session_manage._progressive_summarize()`

---

### 6. `change_model_type(type: str)`

**功能**：切换模型类型（预留函数，当前未实际使用）。

---

## 模块级常量

| 常量 | 值 | 说明 |
|------|-----|------|
| `model_type` | `"default"` | 当前模型类型标识 |
| `response_template` | 多行字符串 | 对话层 JSON 输出格式模板 |

## 依赖关系

| 依赖 | 用途 |
|------|------|
| `langchain_openai.ChatOpenAI` | LLM 客户端 |
| `langchain_core.messages` | 消息类型（System/Human/AI Message） |
| `config.env_config` | 获取 API 配置 |
| `loop.prompt_assemble` | 组装系统提示词 |
| `loop.session_manage` | 获取会话上下文 |
| `tool.base_tool` | 获取工具列表 |

## Tauri 迁移要点

- Rust 中使用 `reqwest` 或 `async-openai` crate 调用 OpenAI 兼容 API
- 消息构造：定义 `enum Message { System(String), Human(String), AI(String) }` 枚举
- JSON 强制输出：通过 `response_format` 参数实现，或使用结构化输出解析
- `response_template` 和 action 系统提示可以定义为 Rust 常量字符串
- 需要实现 JSON 解析的容错处理（对应 Python 的 `parse_llm_json`）
