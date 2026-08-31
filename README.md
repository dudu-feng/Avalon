# Avalon

> 面向个人的轻量智能助手 —— 人性化、低门槛、交互友好。

A lightweight personal intelligent assistant focused on individuals, humanized, low-threshold, and interaction-friendly.

![Version](https://img.shields.io/badge/version-0.4.2-blue)
![Tauri](https://img.shields.io/badge/Tauri-2-orange)
![React](https://img.shields.io/badge/React-19-61dafb)
![Rust](https://img.shields.io/badge/Rust-1.7x-dea584)
![License](https://img.shields.io/badge/license-MIT-green)

## 简介

Avalon 是一个运行在本地的 AI 个人助手，采用 **Tauri 2（Rust 后端 + React 前端）** 构建。它具备多轮对话、会话记忆、定时任务、渠道对接四大核心能力，并内置本地向量化模型（embedding），无需额外部署服务即可开箱即用。

## ✨ 功能特性

- **多轮对话**：ReAct 引擎驱动，流式输出，支持工具调用与中间过程可视化。
- **会话记忆**：自动压缩 + 渐进式总结 + 向量检索（semantic / keyword / hybrid 三模式），长对话不丢上下文。
- **会话管理**：多会话切换、归档、重命名、删除，历史按需渐进式加载。
- **飞书渠道**：长连接接入，私聊直接对话、群聊 @ 触发；思考与工具调用以流式卡片呈现并自动折叠，正文作为独立消息发出；用表情标记处理进度。会话可按聊天隔离，也可全部汇入同一份记忆。
- **主动推送**：模型可通过 `feishu_notify_owner` 主动给你发消息 —— 定时任务的结果因此能落到手机上，而不是只躺在会话文件里。
- **联网搜索**：`web_search` 检索 + `read_web_page` 读取网页正文（AnySearch，默认关闭）。
- **工具沙箱**：文件操作限定在工作区内，终端只能执行白名单命令且不经过 shell。详见下方[安全边界](#-安全边界)。
- **托盘常驻**：关闭窗口不退出进程，定时任务与飞书渠道在后台继续跑。
- **Markdown 渲染**：支持 GFM、代码高亮。
- **定时任务**：让对话自动定时运行（once / daily / weekly），执行历史可继续追问，未读角标提醒。
- **多模型配置**：模型列表增删改、活跃模型一键切换，各模型独立鉴权。
- **本地 embedding**：内置 candle `bge-small-zh-v1.5`，向量化零 API 成本。
- **用量统计**：token 按「天 × 模型」聚合（含缓存命中与思考 token），仪表盘可视化。

## 📸 界面预览

### Chat · 对话

![Chat](assets/Avalon-chat.jpg)

### Schedule · 定时任务

![Schedule](assets/Avalon-schedule.jpg)

### Settings · 设置

![Settings](assets/Avalon-setting.jpg)

## 🧱 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | [Tauri 2](https://tauri.app) |
| 后端 | Rust（tokio · reqwest · candle · serde · chrono） |
| 前端 | React 19 · TypeScript · Vite |
| UI | 自研组件库（CSS Modules，亮/暗双主题）· react-markdown · rehype-highlight |
| 渠道 | 飞书长连接，tokio-tungstenite + prost 手写 pbbp2 私有协议，不依赖第三方 SDK |
| 沙箱 | 自研路径规范化 + 可执行文件解析，直接 spawn 不经 shell |

## 🚀 快速开始

### 环境要求

- **Node.js** ≥ 18（含 npm）
- **Rust**（stable toolchain）
- **Windows**：WebView2（Win10/11 系统自带）

### 开发运行

```bash
cd Avalon-tauri
npm install
npm run tauri dev
```

首次启动会自动在 `Avalon-tauri/` 下生成 `Avalon-config.toml`，在其中填入 LLM 的 API Key 即可对话（默认示例为 DeepSeek，支持任意 OpenAI 兼容接口）。也可以直接复制 `Avalon-config-template.toml` 改名使用 —— 内容与内置模板逐字一致，有测试守着不会漂移。

大部分配置项在应用内「设置」页就能改，改完即时生效；标注「需重启」的除外。

### 接入飞书（可选）

在飞书开放平台创建一个**企业自建应用**（商店应用无法使用长连接），然后：

1. 事件订阅方式选**长连接**，订阅 `im.message.receive_v1`
2. 开通权限：`im:message`、`im:message:send_as_bot`、`cardkit:card:write`（流式卡片）、`im:message.reaction:write`（表情标记）
3. 在应用内「设置 → 渠道」填入 App ID / App Secret，保存后点「启动」

后两项权限缺失不会中断对话，只会退化：没有 `cardkit` 就只剩正文没有思考过程，没有 `reaction` 就没有进度表情。

App Secret 也可用环境变量 `AVALON_FEISHU_APP_SECRET` 覆盖，避免写进配置文件。

启动后**私聊机器人一次**，你的 `open_id` 会被自动记为「主人」，`feishu_notify_owner` 之后就发给你 —— 定时任务的结果推送靠这个。也可以在「设置 → 渠道」里手填 `owner_open_id`。

> ⚠️ 飞书长连接是集群模式且不广播消息：**同一个飞书应用同时只应有一台机器在线**，否则消息会被随机分走。

### 开启联网搜索（可选）

在「设置 → 工具」里把 AnySearch 打开，模型就有了 `web_search` 与 `read_web_page`。API Key 留空也能匿名调用，只是速率受限。

**开关改动需重启应用** —— 工具列表在启动时组装。注意查询词与目标网址会发送到第三方服务，不要用于含密码、密钥的检索。

## 🔒 安全边界

模型能调用文件与终端工具，因此默认带一层沙箱。两条边界对所有来源统一生效 —— 桌面端和飞书渠道用同一套，因为桌面端的模型同样可能被 `read_web_page` 抓回的网页内容注入。

**文件**（`read_file` / `write_file` / `delete_file` / `get_directory_contents`）

只能访问工作区内的路径，读写同一条边界，默认工作区是 `data_root/workspace`。`..` 跳转、NTFS 数据流（`a.txt:hidden`）、Windows 保留设备名（`NUL.txt`）都会被拒绝；符号链接与短名在比对前已被解析，绕不过去。

在「设置 → 工具」里可以改成多个目录，或整个关掉。**放进来的目录，模型就能读也能删** —— 不要加入含密钥、凭证的目录。

**终端**（`run_shell_command`）

只能执行白名单里的命令，默认是 `where` `ping` `ipconfig` `tasklist` `systeminfo` `hostname` 这几个只读诊断命令。

关键在于**命令不经过 shell**：直接 spawn 可执行文件，参数逐个传递，所以 `&&` `|` `>` `;` 没有任何特殊含义，管道和重定向都用不了。这是白名单能成立的前提 —— 交给 `cmd /C` 的话，检查「第一个词是不是 ping」挡不住 `ping x && del /s /q C:\`。

调用形式因此是结构化的：

```json
{ "command": "ping", "args": ["-n", "1", "127.0.0.1"] }
```

只解析 `.exe` / `.com`，`.bat` / `.cmd` / `.ps1` 一律拒绝（它们会重新拉起解释器，等于绕开全部限制），代价是 `npm` / `conda` 这类脚本包装无法调用 —— 这是设计目标不是缺陷。

> ⚠️ **往白名单里加命令前请想清楚这层性质**：一旦放进解释器（`python` / `node` / `powershell`）或带插件机制的工具（`git` 的 `-c core.pager`、`npm` 的 run 脚本），限制就从「防恶意」退化成「防误操作」—— 一行代码即可绕开工作区边界。同理不建议加 `findstr`，它的 `/f:` 参数能读任意文件。

**其它**

- 飞书 `allow_users` 留空 = 不限制。若机器人在有其他成员的组织里，建议填上白名单 —— 否则任何人都能驱动这个有文件和终端权限的助手。
- 敏感项支持环境变量覆盖，避免写进配置文件：`AVALON_LLM_API_KEY`、`AVALON_FEISHU_APP_SECRET`、`ANYSEARCH_API_KEY`。

### 打包桌面安装包

```bash
cd Avalon-tauri
npx tauri build
```

产物：`src-tauri/target/release/bundle/nsis/Avalon_0.4.2_x64-setup.exe`（Windows 安装包，双击安装，默认装到当前用户目录，无需管理员权限）。

## 📁 仓库结构

```
Avalon/
├── Avalon-tauri/        ← 当前主力版本（Tauri 2 桌面应用）✅ 推荐
├── Avalon-python/       ← 早期飞书机器人版本（agent + server）⚠️ 已停止维护
├── Avalon-web/          ← 早期 Vue 3 Web 版本 ⚠️ 已停止维护
├── doc/                 ← 开发设计文档（v0.1 / v0.2 各模块）
└── assets/              ← 界面截图等静态资源
```

> ⚠️ **版本说明**：`Avalon-python/`（飞书机器人）与 `Avalon-web/`（Vue 3 Web 前端）为早期原型，功能已落后且不再维护。其中飞书渠道能力已于 v0.3.0 用 Rust 重写并合入 `Avalon-tauri/`。请使用最新的 **`Avalon-tauri/`** 版本。

## 📄 License

本项目基于 [MIT License](LICENSE) 开源。

- **允许**：自由使用、复制、修改、合并、发布、分发、再许可和/或销售本软件（含商业用途）。
- **条件**：在使用或分发时，必须保留上述版权声明和本许可声明（即标明来源）。
- **免责**：本软件按「现状」提供，不附带任何明示或暗示的担保。
