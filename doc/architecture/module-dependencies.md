# 模块依赖关系

## 1. 依赖关系图

```
main.py
  ├──→ loop.session_manage    (初始化/更新/压缩/保存会话)
  ├──→ loop.react_loop        (执行 ReAct 循环)
  └──→ (无直接依赖 config/llm/tool)

loop.react_loop
  ├──→ llm.llm                (调用对话/动作 LLM)
  ├──→ tool.base_tool         (执行工具调用)
  └──→ (内部函数: parse_llm_json, execute_tool)

llm.llm
  ├──→ config.env_config      (获取 API Key/模型/URL)
  ├──→ loop.prompt_assemble   (组装系统提示词)
  ├──→ loop.session_manage    (获取会话上下文)
  └──→ tool.base_tool         (获取工具列表)

loop.session_manage
  ├──→ config.env_config      (获取路径/阈值配置)
  ├──→ llm.llm                (调用压缩模型)
  ├──→ loop.react_loop        (解析压缩模型 JSON 输出)
  └──→ loop.zvec_store        (写入/删除向量记忆)

loop.zvec_store
  ├──→ config.env_config      (获取向量库路径)
  └──→ loop.embedding_service (文本向量化)

loop.embedding_service
  └──→ config.env_config      (获取模型路径/设备)

loop.prompt_assemble
  └──→ config.env_config      (获取提示词文件路径)

tool.base_tool
  └──→ tool.session_memory_tool (导入搜索工具)

tool.session_memory_tool
  ├──→ loop.zvec_store        (执行向量查询)
  └──→ config.env_config      (获取会话路径)
```

## 2. 依赖矩阵

下表行表示调用方，列表示被调用方。✓ 表示存在依赖。

| 调用方 \ 被调用方 | config | llm | react_loop | session_manage | prompt_assemble | zvec_store | embedding_service | base_tool | session_memory_tool |
|---|---|---|---|---|---|---|---|---|---|
| **main** | | | ✓ | ✓ | | | | | |
| **llm** | ✓ | | | ✓ | ✓ | | | ✓ | |
| **react_loop** | | ✓ | | | | | | ✓ | |
| **session_manage** | ✓ | ✓ | ✓ | | | ✓ | | | |
| **prompt_assemble** | ✓ | | | | | | | | |
| **zvec_store** | ✓ | | | | | | ✓ | | |
| **embedding_service** | ✓ | | | | | | | | |
| **base_tool** | | | | | | | | | ✓ |
| **session_memory_tool** | ✓ | | | | | ✓ | | | |

## 3. 循环依赖分析

存在 **循环依赖链**：

```
llm.llm → session_manage.get_session_context_for_prompt
session_manage → llm.llm_compress
session_manage → react_loop.parse_llm_json
react_loop → llm.llm_chat / llm.llm_action
```

**当前处理方式**：Python 模块级导入 + 函数级调用，运行时不会出现导入错误，但增加了模块耦合度。

**Tauri 迁移建议**：在 Rust 中应通过 trait 抽象打破循环依赖，或引入事件总线模式解耦。

## 4. 模块初始化顺序

```
1. config.env_config        ← 首次导入时加载 .env（无依赖）
2. loop.embedding_service    ← 单例创建（依赖 config，懒加载模型）
3. loop.zvec_store           ← 单例创建（依赖 config + embedding_service）
4. tool.session_memory_tool  ← 函数定义（依赖 zvec_store + config）
5. tool.base_tool            ← 工具注册（导入 session_memory_tool）
6. loop.prompt_assemble      ← 函数定义（依赖 config）
7. llm.llm                   ← 函数定义（依赖 config + prompt_assemble + session_manage + base_tool）
8. loop.react_loop           ← 函数定义（依赖 llm + base_tool）
9. loop.session_manage       ← 函数定义（依赖 config + llm + react_loop + zvec_store）
10. main                     ← 入口（依赖 react_loop + session_manage）
```

> 注意：由于 Python 的动态导入特性，步骤 7-9 实际上存在交叉引用，运行时通过延迟调用解决。Tauri 迁移时需显式处理。
