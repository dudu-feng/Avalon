// 助手正文的 Markdown 渲染组件：把模型输出的 markdown 源文本渲染为富文本。
//
// 基于 react-markdown + remark-gfm（GFM：表格 / 删除线 / 任务列表 / 自动链接）。
// 仅用于 assistant 正文；user 消息保持纯文本（pre-wrap），见 MessageBubble。
// 流式生成时内容为不完整的 markdown，react-markdown 增量解析会有轻微抖动（未闭合标记），
// 属主流聊天应用的固有权衡，可接受。
//
// 代码块右上角提供三态配色切换（经典白 ☀ / 深色 ☾ / 奶油 ☁），
// 状态由 useCodeTheme 全局共享并持久化，所有代码块同步切换。

import type { SVGProps } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import { useCodeTheme } from '../../../hooks/useCodeTheme';
import type { ThemeMode } from '../../../types';
import styles from './MarkdownContent.module.css';

export interface MarkdownContentProps {
  /** markdown 源文本 */
  children: string;
  /** 是否流式生成中（末尾追加闪烁光标） */
  streaming?: boolean;
}

const TITLES: Record<ThemeMode, string> = {
  light: '代码块配色：经典白（点击切换）',
  dark: '代码块配色：深色（点击切换）',
  system: '代码块配色：奶油（点击切换）',
};

function CodeThemeIcon({ mode }: { mode: ThemeMode }) {
  const svgProps: SVGProps<SVGSVGElement> = {
    width: 14,
    height: 14,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 2,
    strokeLinecap: 'round',
    strokeLinejoin: 'round',
  };

  // 经典白 → 太阳
  if (mode === 'light') {
    return (
      <svg {...svgProps} aria-hidden="true">
        <circle cx="12" cy="12" r="5" />
        <line x1="12" y1="1" x2="12" y2="3" />
        <line x1="12" y1="21" x2="12" y2="23" />
        <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
        <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
        <line x1="1" y1="12" x2="3" y2="12" />
        <line x1="21" y1="12" x2="23" y2="12" />
        <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
        <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
      </svg>
    );
  }

  // 深色 → 月亮
  if (mode === 'dark') {
    return (
      <svg {...svgProps} aria-hidden="true">
        <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
      </svg>
    );
  }

  // 奶油（系统）→ 云朵
  return (
    <svg {...svgProps} aria-hidden="true">
      <path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z" />
    </svg>
  );
}

export function MarkdownContent({ children, streaming = false }: MarkdownContentProps) {
  const { codeTheme, cycle } = useCodeTheme();

  return (
    <div className={styles.root}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          pre: (props) => (
            <div className={styles.codeBlock} data-code-theme={codeTheme}>
              <button
                type="button"
                className={styles.themeBtn}
                onClick={cycle}
                aria-label="切换代码块配色"
                title={TITLES[codeTheme]}
              >
                <CodeThemeIcon mode={codeTheme} />
              </button>
              <pre>{props.children}</pre>
            </div>
          ),
        }}
      >
        {children}
      </ReactMarkdown>
      {streaming && <span className={styles.cursor}>▌</span>}
    </div>
  );
}
