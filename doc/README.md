# Avalon 开发文档

> 本文档覆盖 Avalon Python 智能体的全部模块，作为后续 Tauri 应用实现的设计参考。

## 文档导航

### 架构文档 (`architecture/`)

| 文档 | 说明 |
|------|------|
| [整体架构概览](architecture/overview.md) | 项目分层架构、模块职责、技术栈 |
| [模块依赖关系](architecture/module-dependencies.md) | 模块间调用关系图、依赖分析 |
| [数据流程分析](architecture/data-flow.md) | 从用户输入到回答的完整数据流转 |

### 模块文档 (`modules/`)

| 文档 | 模块 | 核心文件 |
|------|------|----------|
| [配置模块](modules/config-module.md) | `config/` | `env_config.py` |
| [LLM 交互模块](modules/llm-module.md) | `llm/` | `llm.py` |
| [核心循环模块](modules/core-loop-module.md) | `loop/` (部分) | `react_loop.py`, `prompt_assemble.py` |
| [会话管理模块](modules/session-module.md) | `loop/` (部分) | `session_manage.py` |
| [向量存储模块](modules/vector-module.md) | `loop/` (部分) | `zvec_store.py`, `embedding_service.py` |
| [工具模块](modules/tool-module.md) | `tool/` | `base_tool.py`, `session_memory_tool.py` |

### 接口文档 (`interfaces/`)

| 文档 | 说明 |
|------|------|
| [对外接口参考](interfaces/api-reference.md) | 各模块对外开放的函数、类、接口签名 |

### 迁移文档 (`tauri-migration/`)

| 文档 | 说明 |
|------|------|
| [Tauri 迁移指南](tauri-migration/migration-guide.md) | Python → Tauri(Rust) 模块映射、实现建议 |

## 项目源码结构

```
Avalon-python/agent/
├── main.py                     # 程序入口，CLI 交互循环
├── __init__.py
├── .env                        # 环境变量配置文件
├── config/
│   └── env_config.py           # 环境变量配置中心（单例）
├── llm/
│   ├── __init__.py
│   └── llm.py                  # LLM 调用封装（对话/动作/压缩）
├── loop/
│   ├── __init__.py
│   ├── chat_init.py            # （空文件，预留）
│   ├── react_loop.py           # ReAct 循环引擎
│   ├── prompt_assemble.py      # 系统提示词组装
│   ├── session_manage.py       # 会话管理与压缩
│   ├── embedding_service.py    # Embedding 向量化服务
│   └── zvec_store.py           # ZVec 向量数据库存储
└── tool/
    ├── __init__.py
    ├── base_tool.py            # 基础工具定义（文件操作/Shell）
    └── session_memory_tool.py  # 会话记忆检索工具
```
