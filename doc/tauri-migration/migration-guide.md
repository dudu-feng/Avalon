# Tauri 迁移指南

> 本文档将 Python 智能体的各模块映射到 Tauri (Rust + Frontend) 架构，提供实现建议。

## 1. 整体架构映射

### Python → Tauri 架构对照

| Python 层 | Tauri 对应 | 说明 |
|-----------|-----------|------|
| `main.py` (CLI 交互) | Frontend (React) | UI 交互层，通过 Tauri IPC 调用后端 |
| `llm/` (LLM 调用) | Rust 后端 (`src-tauri/src/llm/`) | HTTP 调用 LLM API |
| `loop/react_loop.py` (ReAct 循环) | Rust 后端 (`src-tauri/src/loop/`) | 核心循环引擎 |
| `loop/session_manage.py` (会话管理) | Rust 后端 (`src-tauri/src/session/`) | 会话 CRUD + 压缩 |
| `loop/zvec_store.py` (向量存储) | Rust 后端 (`src-tauri/src/vector/`) | ZVec Rust SDK |
| `loop/embedding_service.py` (Embedding) | Rust 后端 (`src-tauri/src/embedding/`) | candle / ort |
| `tool/` (工具) | Rust 后端 (`src-tauri/src/tools/`) | Tool trait + 实现 |
| `config/env_config.py` (配置) | Rust 后端 (`src-tauri/src/config/`) | Tauri 配置体系 |
| `.env` (环境变量) | `tauri.conf.json` + 配置文件 | Tauri 配置管理 |

### 建议的 Rust 模块结构

```
src-tauri/src/
├── main.rs              # Tauri 入口
├── lib.rs               # 模块导出
├── config/
│   └── mod.rs           # 配置中心（读取配置文件）
├── llm/
│   └── mod.rs           # LLM 调用（chat/action/compress）
├── loop/
│   ├── mod.rs           # ReAct 循环引擎
│   └── prompt.rs        # 提示词组装
├── session/
│   └── mod.rs           # 会话管理 + 压缩 + 渐进式总结
├── vector/
│   ├── zvec_store.rs    # ZVec 向量数据库
│   └── embedding.rs     # Embedding 服务
├── tools/
│   ├── mod.rs           # Tool trait + 注册表
│   ├── file_ops.rs      # 文件操作工具
│   ├── shell.rs         # Shell 命令工具
│   └── memory_search.rs # 会话记忆检索
└── commands.rs          # Tauri IPC 命令定义
```

## 2. 模块迁移详解

### 2.1 配置模块

**Python**：`.env` 文件 + `EnvConfig` 单例

**Rust 方案**：
```rust
use std::sync::OnceLock;
use serde::Deserialize;

#[derive(Deserialize)]
struct AppConfig {
    default_api_key: String,
    default_model: String,
    default_model_base_url: String,
    // ... 其他字段
}

static CONFIG: OnceLock<AppConfig> = OnceLock::new();

pub fn config() -> &'static AppConfig {
    CONFIG.get_or_init(|| {
        // 从 Tauri 配置目录读取 config.json
        let path = tauri::api::path::app_config_dir(&config)
            .unwrap().join("config.json");
        let content = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    })
}
```

**要点**：
- 用 `OnceLock` 替代 Python 单例
- `.env` 替换为 `config.json`（更结构化）
- 路径用 Tauri 的 `app_data_dir()` / `app_config_dir()`
- 阈值解析（`"64k"` → 64000）需手动实现

### 2.2 LLM 交互模块

**Python**：LangChain `ChatOpenAI`

**Rust 方案**：直接 HTTP 调用 OpenAI 兼容 API
```rust
use reqwest::Client;
use serde::{Serialize, Deserialize};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

pub async fn llm_chat(user_input: &str, chat_history: &[Value]) -> Result<ChatResponse> {
    let client = Client::new();
    let messages = build_messages(user_input, chat_history);
    let resp = client
        .post(&format!("{}/chat/completions", config().default_model_base_url))
        .bearer_auth(&config().default_api_key)
        .json(&ChatRequest {
            model: config().default_model.clone(),
            messages,
            response_format: None,
        })
        .send().await?
        .json::<ChatResponse>().await?;
    Ok(resp)
}
```

**要点**：
- `reqwest` 替代 LangChain（Rust 无等效框架）
- 消息类型用 enum：`enum Message { System(String), Human(String), AI(String) }`
- JSON 强制输出：`response_format: {"type": "json_object"}`
- `token_usage` 从 `usage` 字段提取

### 2.3 核心循环模块

**Python**：`while True` 双层循环

**Rust 方案**：
```rust
pub async fn react_loop(user_input: &str) -> Vec<ChatEntry> {
    let mut history = vec![ChatEntry::user(user_input)];
    loop {
        let result = llm::llm_chat(user_input, &history).await;
        let parsed = parse_llm_json(&result.content);

        match parsed.get("next").and_then(|n| n.as_str()) {
            Some("stop") => {
                history.push(transform_chat(&parsed, &result));
                break;
            }
            Some("action") => {
                let action_history = run_action_loop(user_input, &parsed, &mut history).await;
                history.push(ChatEntry::action_record(action_history));
            }
            _ => {
                // JSON 解析失败，纯文本回复
                history.push(ChatEntry::assistant(&result.content));
                break;
            }
        }
    }
    history
}
```

**要点**：
- `loop {}` + `match` 替代 `while True` + `if`
- 异步：所有 LLM/IO 操作用 `async/await`
- JSON 解析容错：`parse_llm_json` 需处理 Markdown 代码块剥离
- 工具执行：通过 trait 动态分发

### 2.4 会话管理模块

**Python**：JSON 文件读写 + 压缩逻辑

**Rust 方案**：
```rust
#[derive(Serialize, Deserialize)]
struct Session {
    id: String,
    status: String,
    compress_round: u32,
    compressed: Vec<CompressedChunk>,
    super_compressed: Option<SuperChunk>,
    session: Vec<ChatEntry>,
}

pub fn session_compress() -> Result<()> {
    let mut current = get_current_session()?;
    if current.session.is_empty() { return Ok(()); }

    let compressed = llm::llm_compress(&current).await?;
    let parsed = parse_llm_json(&compressed.content);

    current.compress_round += 1;
    current.compressed.push(parsed);
    // 归档原始数据
    archive_raw(&current)?;
    current.session.clear();
    save_current_session(&current)?;

    // ZVec 同步
    zvec_store::insert_session_memory(&doc_id, &summary, &keywords, &timestamp)?;

    // 渐进式总结
    progressive_summarize()?;
    Ok(())
}
```

**要点**：
- 用 `serde` 定义强类型 struct
- 递归 token 统计需遍历嵌套 `action_history`
- 渐进式总结的块编号解析逻辑需移植
- 文件 I/O 用 `std::fs`，注意并发安全

### 2.5 向量存储模块

**Python**：ZVec Python SDK + sentence-transformers

**Rust 方案**：
- ZVec 有 Rust 原生 SDK，可直接使用
- Embedding 模型用 `candle-core` 或 `ort` (ONNX Runtime)

```rust
// Embedding
use candle_core::{Device, Tensor};
use candle_transformers::models::bge;

pub struct EmbeddingService {
    model: OnceLock<bge::Model>,
}

impl EmbeddingService {
    pub fn doc_embedding(&self, text: &str) -> Vec<f32> {
        let model = self.model.get_or_init(|| load_model());
        let tokens = tokenize(text);
        let embedding = model.forward(&tokens).unwrap();
        normalize_l2(&embedding)
    }

    pub fn query_embedding(&self, text: &str) -> Vec<f32> {
        let instruction = "为这个句子生成表示以用于检索相关文章：";
        self.doc_embedding(&format!("{}{}", instruction, text))
    }
}
```

**要点**：
- 512 维向量用 `Vec<f32>`
- L2 归一化手动实现
- bge 模型 query/doc 非对称检索（query 需指令前缀）
- 考虑模型加载耗时，需懒加载

### 2.6 工具模块

**Python**：LangChain `@tool` 装饰器

**Rust 方案**：
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn invoke(&self, args: serde_json::Value) -> String;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn execute(&self, name: &str, args: Value) -> String {
        self.tools.iter()
            .find(|t| t.name() == name)
            .map(|t| t.invoke(args))
            .unwrap_or_else(|| format!("未找到工具: {}", name))
    }

    pub fn get_tool_list(&self) -> String {
        self.tools.iter()
            .map(|t| format!("- **{}**: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
```

**要点**：
- `trait Tool` + `Vec<Box<dyn Tool>>` 替代 `@tool` 装饰器
- Shell 命令：`std::process::Command`，注意超时控制
- **安全增强**：路径白名单、命令黑名单、资源限制
- `search_session_memory` 异步调用 zvec_store

## 3. Tauri IPC 接口设计

### 前端 → 后端命令

```rust
#[tauri::command]
async fn send_message(user_input: String) -> Result<Vec<ChatEntry>, String> {
    let history = loop::react_loop(&user_input).await;
    session::update_current_session(&history);
    session::auto_compress_check(&history).await;
    Ok(history)
}

#[tauri::command]
async fn compress_session() -> Result<(), String> {
    session::session_compress().await
}

#[tauri::command]
async fn init_session() -> Result<(), String> {
    session::init_session()
}

#[tauri::command]
async fn get_session_list() -> Result<Vec<SessionMeta>, String> {
    session::get_history_sessions()
}

#[tauri::command]
async fn search_memory(query: String, mode: String, topk: u32) -> Result<String, String> {
    tools::search_session_memory(&query, &mode, topk, "")
}
```

### 后端 → 前端事件

```rust
// 流式输出 LLM 回复
app_handle.emit("llm_message", &message)?;

// 工具调用通知
app_handle.emit("tool_call", &{ tool_name, arguments })?;

// 工具结果通知
app_handle.emit("tool_result", &result)?;

// 压缩进度
app_handle.emit("compress_started", ())?;
app_handle.emit("compress_finished", &{ chunks: N })?;
```

## 4. 数据兼容性

### 会话文件格式

保持 JSON 格式不变，确保 Python 版和 Tauri 版的数据互操作：

```rust
#[derive(Serialize, Deserialize)]
struct Session {
    id: String,           // "terminal_2026-06-06-23_03_31"
    status: String,       // "active" / "archived" / "inactive"
    compress_round: u32,
    compressed: Vec<CompressedChunk>,
    super_compressed: Option<SuperChunk>,
    session: Vec<ChatEntry>,
}
```

### 目录结构

保持与 Python 版相同的目录布局：
```
{app_data_dir}/
├── session/
│   ├── current/terminal.json
│   └── history/terminal/{session_id}/
│       ├── index.json
│       └── raw/{chunk}.json
├── vector/
│   └── zvec/             # ZVec 数据库
└── prompt/               # Markdown 提示词文件
```

## 5. 迁移优先级建议

| 阶段 | 模块 | 优先级 | 说明 |
|------|------|--------|------|
| 1 | 配置模块 | 高 | 基础设施，其他模块依赖 |
| 2 | LLM 交互模块 | 高 | 核心功能，无外部依赖 |
| 3 | 核心循环模块 | 高 | 依赖 LLM + 工具 |
| 4 | 工具模块（基础工具） | 高 | 文件/Shell 操作 |
| 5 | 会话管理模块 | 中 | 依赖 LLM + 向量 |
| 6 | 向量存储模块 | 中 | 依赖 Embedding |
| 7 | 会话记忆检索工具 | 中 | 依赖向量存储 |
| 8 | 渐进式总结 | 低 | 依赖会话管理完整 |

## 6. 注意事项

1. **异步处理**：Rust 中所有 IO/网络操作需 `async/await`，与 Python 同步模型不同
2. **错误处理**：Python 的 `try/except` → Rust 的 `Result<T, E>` + `?` 运算符
3. **JSON 解析**：`serde_json` 替代 `json` 模块，需定义完整类型
4. **单例模式**：`OnceLock` / `LazyLock` 替代 Python `__new__` 单例
5. **LLM 流式输出**：Tauri 版可实现 SSE 流式响应，优于 Python 版的阻塞式调用
6. **安全性**：Tauri 桌面应用需增加路径校验、命令过滤等安全措施
7. **Embedding 模型**：Rust 生态的模型加载方案需验证兼容性（bge-small-zh-v1.5）
8. **循环依赖**：Python 版存在 llm ↔ session_manage ↔ react_loop 循环依赖，Rust 中通过 trait 抽象或模块重组打破
