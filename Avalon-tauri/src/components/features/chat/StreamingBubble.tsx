// 正在流式生成的 assistant 气泡：整个应用里唯一订阅 streaming 切片的组件。
//
// 增量只更新 store.state.streaming，所以每次 flush 只会重渲染这里（以及它内部的
// MarkdownContent），已提交的 MessageList、输入框、工具栏都不参与。
// 内容增长时通过 onGrow 通知 MessageList 跟随滚动 —— 这样 MessageList 自己
// 不需要订阅高频状态。

import { useEffect } from 'react';
import type { ChatMessage } from '../../../types/chat';
import { notePainted } from '../../../lib/streamPerf';
import { MessageBubble } from './MessageBubble';
import { useChatStreaming } from './useChat';

export interface StreamingBubbleProps {
  /** 当前来源筛选是否包含这条流式消息；false 时仍保持订阅，但不渲染 */
  visible?: boolean;
  /** 内容增长 / 气泡首次出现时触发（MessageList 用于跟随滚动） */
  onGrow?: () => void;
  /** 流式气泡出现 / 消失时通知列表（用于空态与滚动模式判断） */
  onPresenceChange?: (present: boolean) => void;
}

export function StreamingBubble({
  visible = true,
  onGrow,
  onPresenceChange,
}: StreamingBubbleProps) {
  const streaming = useChatStreaming();
  const present = streaming !== null;
  // 被来源筛选隐藏时，对列表而言等同于没有流式气泡（空态 / 滚动模式都按可见性判断）
  const visiblePresent = present && visible;

  // 存在性变化很低频（每轮或筛选切换一次），不会造成列表跟随增量重渲染
  useEffect(() => {
    onPresenceChange?.(visiblePresent);
  }, [visiblePresent, onPresenceChange]);

  // 每次增量都会走到这里：只在用户本来就贴着底部时跟随
  const growth = streaming ? `${streaming.id}:${streaming.thought.length}:${streaming.content.length}` : '';
  useEffect(() => {
    if (present && visible) onGrow?.();
    notePainted(); // dev-only：useEffect 在绘制之后执行
  }, [growth, present, visible, onGrow]);

  if (!streaming || !visible) return null;

  // 复用普通气泡的样式与渲染逻辑；桌面来源不显示来源徽章，保持和以前一致
  const message: ChatMessage = {
    id: streaming.id,
    role: 'assistant',
    status: 'streaming',
    thought: streaming.thought,
    content: streaming.content,
    origin: streaming.source === 'desktop' ? null : { source: streaming.source },
  };

  return <MessageBubble message={message} />;
}
