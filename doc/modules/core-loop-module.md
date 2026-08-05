# 核心循环模块 (`loop/react_loop.py`, `loop/prompt_assemble.py`)

## 概述

核心循环模块实现 Avalon 的 **双层 ReAct 循环引擎**，是整个智能体的决策中枢。同时包含系统提示词的组装逻辑。

## 文件清单

| 文件 | 说明 |
|------|------|
| `loop/react_loop.py` | ReAct 双层循环引擎、工具执行、JSON 解析 |
| `loop/prompt_assemble.py` | 系统提示词加载与组装 |

---

## react_loop.py

### 核心函数

#### 1. `react_loop(user_input: str) -> dict`

**功能**：ReAct 循环主入口。接收用户输入，执行对话层+动作层循环，返回完整聊天历史。

**实现逻辑**：
```
1. 初始化 chat_history = [{"role":"user", "content":user_input}]
2. 进入对话层循环 (while True):
   a. 调用 llm.llm_chat(user_input, chat_history)
   b. 解析 JSON: parse_llm_json(result.content)
   c. 若解析失败 → 纯文本回复，返回
   d. 输出 message 给用户
   e. 追加 assistant 消息到 chat_history
   f. 若 next=="stop" → break
   g. 若 next=="action" → 进入动作层循环
3. 返回 chat_history
```

**动作层循环逻辑**：
```
1. 初始化 action_history = [{"action_target": target}]
2. while True:
   a. 调用 llm.llm_action(user_input, action_target, action_history)
   b. 解析 JSON
   c. 根据 next 分支:
      - "tool_call":    执行工具 → 追加结果 → continue
      - "sub_analysis": 追加分析 → continue
      - "finished":     追加完成记录 → break
      - 其他:           追加错误 → break
3. 将 action_history 追加到 chat_history
```

**返回值结构**：
```python
[
    {"role": "user", "time": "...", "content": "用户输入"},
    {"role": "assistant", "time": "...", "content": "回复",
     "thought": "思考", "token_usage": {...}},
    # 若有 action:
    {"role": "assistant", "content": "【执行记录】",
     "action_history": [
         {"action_target": "..."},
         {"step": "tool_call", "time": "...", "analysis": "...",
          "action": {...}, "token_usage": {...}},
         # ...
     ]}
]
```

**调用方**：`main.py`

---

#### 2. `execute_tool(tool_name: str, arguments: dict) -> str`

**功能**：根据工具名称执行对应工具，返回执行结果字符串。

**实现逻辑**：
1. 遍历 `base_tool.TOOLS` 列表
2. 匹配 `tool.name == tool_name`
3. 调用 `tool.invoke(arguments)`
4. 异常捕获返回错误信息
5. 未找到工具返回 `"未找到工具: {tool_name}"`

**调用方**：`react_loop()` 动作层循环

---

#### 3. `parse_llm_json(llm_output_content: str) -> dict`

**功能**：解析 LLM 返回的 JSON 内容，兼容 Markdown 代码块包裹。

**实现逻辑**：
1. 空值或非字符串检查 → 返回 `{}`
2. 去除 Markdown 代码块标记（` ```json ` 和 ` ``` `）
3. `json.loads()` 解析
4. 解析失败 → 返回 `{}`
5. 非 dict 类型 → 返回 `{}`

**调用方**：`react_loop()`, `session_manage.session_compress()`, `session_manage._progressive_summarize()`

---

#### 4. `chat_result_transform(chat_result_content: dict, chat_result) -> dict`

**功能**：将对话层 LLM 结果转换为标准聊天历史格式。

**输出结构**：
```python
{
    "role": "assistant",
    "time": "YYYY-MM-DD-HH:MM:SS",
    "content": "回复消息",
    "thought": "思考过程",
    "token_usage": {"input_tokens": N, "output_tokens": N, ...}
}
```

---

#### 5. `action_result_transform(action_result_content: dict, action_result) -> dict`

**功能**：将动作层 LLM 结果转换为标准动作历史格式。

**输出结构**：
```python
{
    "step": "tool_call/sub_analysis/finished",
    "time": "YYYY-MM-DD-HH:MM:SS",
    "analysis": "分析内容",
    "action": {...},  # 根据 next 取对应字段
    "token_usage": {...}
}
```

---

#### 6. `get_current_time() -> str`

**功能**：返回当前时间字符串，格式 `"%Y-%m-%d-%H:%M:%S"`。

---

### prompt_assemble.py

#### `assemble_system_prompt() -> list`

**功能**：加载所有 Markdown 提示词文件，组装系统提示词列表。

**实现逻辑**：
1. 获取提示词目录路径（优先 `.env` 配置，回退到 `agent/config/prompt/`）
2. 初始化 `prompt_list`，包含基本设定文本（Avalon 身份描述）
3. 遍历目录下所有 `.md` 文件
4. 逐个加载文件内容，追加到 `prompt_list`
5. 返回 `prompt_list`（字符串列表）

**基本设定内容**：
```
你是智能体Avalon,是由dudu-feng开发的一款智能体
Avalon是传说中遗世独立的理想乡
"make your own Avalon"——"创造属于你自己的Avalon"
```

**调用方**：`llm.llm_chat()`

---

#### `load_prompt(file_name: str) -> str`

**功能**：加载单个提示词文件内容。

**实现逻辑**：读取文件，`.strip()` 去除首尾空白。

---

## 依赖关系

| react_loop 依赖 | 用途 |
|-----------------|------|
| `tool.base_tool` | 工具列表、工具执行 |
| `llm.llm` | LLM 调用 |

| prompt_assemble 依赖 | 用途 |
|----------------------|------|
| `config.env_config` | 提示词文件路径 |

## Tauri 迁移要点

- ReAct 循环：Rust 中用 `loop {}` + `match` 实现双层循环
- JSON 解析：使用 `serde_json`，需处理 Markdown 代码块剥离
- 提示词组装：遍历目录用 `std::fs::read_dir`，过滤 `.md` 文件
- 工具执行：通过 trait + 动态分发实现工具调用
- `token_usage` 提取：从 LLM API 响应中解析 `usage` 字段
