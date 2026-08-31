# Avalon-tauri

Avalon 的主力版本（Tauri 2 桌面应用）。**项目介绍、功能说明、安装与配置请看[仓库根目录的 README](../README.md)。**

本文件只记录在这个子目录里干活时用得上的东西。

## 常用命令

```bash
npm install              # 安装前端依赖
npm run tauri dev        # 开发运行（前后端一起起）
npx tauri build          # 打包 Windows 安装包
npx tsc --noEmit         # 前端类型检查

cd src-tauri
cargo test               # 后端单元测试
cargo build              # 只编译后端
```

打包细节（图标、NSIS 配置、产物路径、常见报错）见 [build_readme.md](build_readme.md)。

## 目录结构

```
Avalon-tauri/
├── src/                      前端（React 19 + TypeScript）
│   ├── pages/                页面：Chat / Schedule / Dashboard / Settings / About
│   ├── components/
│   │   ├── ui/               自研基础组件库（CSS Modules，亮暗双主题）
│   │   ├── features/         业务组件
│   │   ├── layout/           外壳与导航
│   │   └── icons/
│   ├── lib/                  Tauri 命令的前端封装（*Api.ts）
│   ├── hooks/
│   ├── styles/               设计令牌与全局样式
│   └── types/                与后端 serde 结构对应的类型定义
├── src-tauri/                后端（Rust）
│   └── src/
│       ├── engine/           ReAct 循环编排
│       ├── llm/              OpenAI 兼容客户端与流式解析
│       ├── tool/             工具层（文件/终端/记忆/搜索/飞书/定时）+ sandbox
│       ├── session/          会话存储、压缩、渐进式总结
│       ├── vector/           向量索引
│       ├── embedding/        本地 candle embedding
│       ├── channel/          飞书长连接渠道
│       ├── scheduler/        定时任务
│       ├── config/           配置加载/保存/校验
│       └── test_file/        测试
├── Avalon-config.toml          实际配置（gitignore，含密钥）
└── Avalon-config-template.toml 分发模板（由 loader.rs 的 DEFAULT_TEMPLATE 生成）
```

## 几条容易踩的约定

- **配置模板有两份，但只有一个来源。** `Avalon-config-template.toml` 必须与 `src-tauri/src/config/loader.rs` 里的 `DEFAULT_TEMPLATE` 逐字一致（仅首行说明注释除外），改了后者就要同步前者 —— `config_test` 里有测试会拦住不一致的提交。这个文件历史上漂移过三次。
- **模板里不能出现真实密钥。** 它是 git 追踪的，写进去就等于提交进历史，删掉也不会消失。同样有测试拦着。
- **配置里的 Windows 路径要用正斜杠。** `\A`、`\d` 在 TOML 里是非法转义，写进去整份配置会解析失败，下次启动被兜底成默认配置。设置页的目录选择器已经自动做了转换。
- **密钥绝不能进日志。** 见 `logging.rs` 头部注释。
- **新增工具失败消息时，要同步 `engine/react.rs` 的 `FAIL_PREFIXES`**，否则前端不会把它标成失败。
