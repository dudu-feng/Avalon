# Avalon Tauri 前端编码规范

> 本文档约定 Avalon 桌面端前端的目录结构、组件写法、样式、类型与接口封装规则，
> 用于后续代码编写时保持风格统一。所有新代码应遵循本规范。

## 1. 技术栈

| 层 | 选型 |
|---|---|
| 框架 | React 19 + TypeScript 6 |
| 构建 | Vite 8（`@vitejs/plugin-react`） |
| 桌面壳 | Tauri 2（`@tauri-apps/api` + `plugin-opener`） |
| 样式 | CSS Modules + CSS 自定义属性（design tokens） |
| 组件库 | 自研（不引入第三方 UI 库） |

---

## 2. 目录结构

```
src/
├── main.tsx               入口：挂载 React，注入 tokens.css + global.css
├── App.tsx                装配器：只做「壳 + 当前页面」的组装
├── components/
│   ├── layout/            应用壳层（MainLayout / Sidebar / Header / MenuItem）
│   ├── ui/                原子组件（Button / Input / Card / Badge / PageContainer / ThemeToggle）
│   ├── features/          业务复合组件（chat/ 等，按功能域分目录）
│   └── icons/             内联 SVG 图标（createIcon 工厂）
├── pages/                 页面：注册表 + 每页一目录
├── hooks/                 通用 hooks（useTheme 等）
├── lib/                   接口封装 + 工具函数（chatApi / llmParser）
├── styles/                全局样式（tokens.css / global.css）
├── types/                 类型定义（按域拆分）
└── assets/                静态资源（logo 等）
```

---

## 3. 分层原则（核心）

| 层 | 定位 | 边界 |
|---|---|---|
| `components/ui` | 无业务含义的原子组件 | 不 import 业务类型、不调用后端接口 |
| `components/features` | 页面专属复合组件 | 可组合 ui 组件 + 调用 lib/hooks |
| `components/layout` | 应用壳 | 只负责整体骨架 |
| `pages` | 页面装配 | 组合 features 组件，最薄一层 |
| `lib` | 后端接口封装 + 纯工具 | 唯一的 `invoke` 出口 |
| `hooks` | 可复用逻辑 | 状态机 / 副作用 |

**规则：**
- 组件（ui / features / layout）**不得直接 `invoke`**，一律走 `lib` 层封装。
- 页面（pages）**只做组装**，业务逻辑下放到 features 组件或 hooks。
- ui 组件保持通用，页面专属逻辑不污染 ui 层。

---

## 4. 组件编写规范

### 4.1 命名与文件

- 组件文件名 `PascalCase.tsx`，样式 `PascalCase.module.css`，目录 `PascalCase/`。
- 每个组件目录含一个 `index.ts` 做 barrel 导出。

### 4.2 导出方式

统一使用**具名导出** + barrel 统一 re-export，不使用默认导出（`App` 除外）。

```tsx
// components/ui/Button.tsx
export function Button({ ... }: ButtonProps) { ... }
```

```ts
// components/ui/index.ts
export { Button } from './Button';
export type { ButtonProps } from './Button';
```

### 4.3 Props 定义

- props 类型命名 `XxxProps`，随组件一起导出。
- 基础组件继承原生元素属性类型，variant 用**联合类型**收敛到 `types/`。

```tsx
import type { ButtonHTMLAttributes } from 'react';
import type { ButtonVariant, ButtonSize } from '../../types';
import styles from './Button.module.css';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export function Button({
  variant = 'primary',
  size = 'md',
  className = '',
  children,
  ...rest
}: ButtonProps) {
  const classes = [styles.button, styles[variant], styles[size], className]
    .filter(Boolean)
    .join(' ');

  return (
    <button type="button" className={classes} {...rest}>
      {children}
    </button>
  );
}
```

### 4.4 组件模板

```tsx
import type { ... } from 'react';
import styles from './Xxx.module.css';

export interface XxxProps {
  // ...
}

export function Xxx({ ... }: XxxProps) {
  return <div className={styles.xxx}>...</div>;
}
```

---

## 5. 样式规范

### 5.1 CSS Modules

- 一律使用 `*.module.css`，通过 `styles.xxx` 引用。
- 类名使用 camelCase。
- 需要命中全局类（如 `html.dark`）时用 `:global()`：

```css
.logoDark { display: none; }
:global(.dark) .logo { display: none; }
:global(.dark) .logoDark { display: block; }
```

### 5.2 设计 token

- **颜色、字号、圆角、阴影、间距一律引用 token，禁止硬编码。**

```css
.good {
  color: var(--foreground);
  background: var(--popover);
  border-radius: var(--radius);
  font: 600 14px/1.1 var(--font-sans);
}

.bad {
  color: #333;
  border-radius: 12px;
}
```

- token 定义在 `styles/tokens.css`（primitive + semantic 双层），组件只引用 **semantic 层**（`--foreground` / `--primary` / `--muted` 等）。
- 常用变量：`--background` `--foreground` `--card` `--popover` `--muted` `--muted-foreground` `--primary` `--primary-foreground` `--secondary` `--border` `--ring` `--destructive` `--radius` `--radius-sm` `--font-sans` `--font-serif` `--font-display` `--font-mono`。

### 5.3 主题

- 主题由 `html.dark` 类驱动（`hooks/useTheme.ts` 挂载），组件**不感知主题状态**。
- 需要明暗切换的元素，用 CSS 变量 + `:global(.dark)` 处理，不要用 JS 判断主题。

### 5.4 响应式

- 统一断点 `640px`（`@media (max-width: 640px)`）。

---

## 6. 类型规范

- 类型集中放在 `src/types/`，按域拆分：`index.ts`（基础 variant）、`chat.ts`（聊天域）。
- variant / 联合类型收敛在 types 目录，组件里只 import。

```ts
// types/index.ts
export type ButtonVariant = 'primary' | 'secondary' | 'ghost';
export type ThemeMode = 'light' | 'dark' | 'system';
```

---

## 7. 接口封装规范（lib 层）

### 7.1 封装 `invoke`

- 所有后端调用封装在 `lib/` 下，组件只调语义化函数。

```ts
// lib/chatApi.ts
import { invoke } from '@tauri-apps/api/core';
import type { LlmResponse } from '../types/chat';

export type ChatParams = {
  systemPrompt: string;
  userInput: string;
  chatHistory: string;
};

export async function llmChat(params: ChatParams): Promise<LlmResponse> {
  return invoke<LlmResponse>('llm_chat', params);
}
```

> ⚠️ **注意**：`invoke` 的参数类型是 `Record<string, unknown>`，
> 参数类型必须用 **`type`（对象字面量类型）而非 `interface`**，
> 否则会报 `Index signature for type 'string' is missing`。

### 7.2 参数命名映射

- Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射：
  - Rust `system_prompt` ↔ JS `systemPrompt`
  - Rust `chat_history` ↔ JS `chatHistory`

### 7.3 后端未就绪的占位模式

- 后端尚未完善时，接口封装写好后，在调用侧用 `USE_MOCK` 开关占位，后端完善后置 `false` 即切回真实调用。
- 参考 `components/features/chat/useChat.ts`：

```ts
const USE_MOCK = true; // 后端完善后改为 false

if (!USE_MOCK) {
  const res = await llmChat({ ... });
  // 真实逻辑
}
// 占位逻辑
```

---

## 8. Hooks 规范

- 命名 `useXxx`，返回 `{ 状态, 动作 }` 结构。
- 状态机、副作用、后端调用逻辑放 hook，组件保持薄。

```ts
export function useChat(options: UseChatOptions = {}) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  // ...
  return { messages, isBusy, send, clear };
}
```

---

## 9. 页面搭建规范

### 9.1 页面注册表（唯一导航配置源）

`pages/index.tsx` 集中维护「菜单项 + 页面组件」的映射，Sidebar / Header / 内容区都从它派生。

```tsx
export interface PageConfig extends MenuItemData {
  component: ComponentType;
}

export const pages: PageConfig[] = [
  { id: 'home', label: 'Home', icon: <HomeIcon />, component: HomePage },
  // 新增页面在这里加一项
];
```

### 9.2 页面组装

```tsx
// pages/ChatPage/ChatPage.tsx —— 最高层，只做组装
export function ChatPage() {
  const { messages, isBusy, send } = useChat();
  return (
    <div className={styles.chat}>
      <MessageList messages={messages} />
      <ChatInput onSubmit={send} disabled={isBusy} />
    </div>
  );
}
```

### 9.3 布局选择

- **文档/表单类页面**：用 `PageContainer`（统一 `max-width` + 标题区）。
- **聊天/画布类页面**：自定义布局（全宽、内部滚动、固定底部），复用 `MainLayout` 壳即可。

---

## 10. 新增内容 Checklist

### 新增一个页面

1. 在 `pages/XxxPage/` 建 `XxxPage.tsx` + `XxxPage.module.css` + `index.ts`。
2. 在 `pages/index.tsx` 注册表中加一项（含 icon + component）。
3. 如需新图标，在 `components/icons/index.tsx` 用 `createIcon` 加一个。

### 新增一个 UI 组件

1. 在 `components/ui/Xxx.tsx` + `Xxx.module.css`，具名导出 + 导出 `XxxProps`。
2. 在 `components/ui/index.ts` 加 barrel 导出。
3. variant 类型收敛到 `types/`。

### 新增一个业务组件

1. 在 `components/features/<域>/` 下建组件 + 样式 + 局部 `index.ts`。
2. 若涉及后端调用，走 `lib/` 封装，不直接 `invoke`。

### 新增后端接口封装

1. 在 `lib/xxxApi.ts` 用 `invoke` 封装，参数用 `type` 定义。
2. 返回类型在 `types/` 定义。

---

## 11. 当前状态备注

- **后端未完善**：`llm_chat` / `llm_action` / `llm_compress` 命令已注册，但返回内容与工具执行链路尚未完整。
- **`greet` 命令已移除**：旧模板的 `greet` 演示在 `pages/HomePage` 中仍引用，后端完善后需一并处理。
- **未使用的依赖**：`antd`、`@xyflow/react` 已声明但当前未使用（走自研组件）。
