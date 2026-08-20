import { useEffect, useLayoutEffect, useRef } from 'react';
import type { ChatMessage } from '../../../types/chat';
import { MessageBubble } from './MessageBubble';
import { ScrollArea, Skeleton } from '../../ui';
import styles from './MessageList.module.css';

export interface MessageListProps {
  messages: ChatMessage[];
  /** 加载中（初始拉取历史 / 新会话处理中）：显示骨架屏占位 */
  loading?: boolean;
}

export function MessageList({ messages, loading = false }: MessageListProps) {
  const endRef = useRef<HTMLDivElement>(null);
  const initializedRef = useRef(false);

  // 是否接近底部（用于对话更新时决定是否跟随滚动，避免打断用户上滚阅读）
  const nearBottom = () => {
    const vp = endRef.current?.parentElement;
    if (!vp) return true;
    return vp.scrollHeight - vp.scrollTop - vp.clientHeight < 120;
  };

  // 首次加载历史：在绘制前 instant 定位到底，消除「先渲染顶部再滚到底」的闪烁与慢滚动
  useLayoutEffect(() => {
    if (initializedRef.current || messages.length === 0) return;
    endRef.current?.scrollIntoView({ block: 'end' });
    initializedRef.current = true;
  }, [messages]);

  // 对话更新：仅当用户接近底部时才平滑跟随（上滚阅读历史时保持原位）
  useEffect(() => {
    if (!initializedRef.current) return;
    if (nearBottom()) endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [messages]);

  if (loading) {
    return (
      <div className={styles.skeletonList} aria-hidden="true">
        <div className={styles.skeletonRowRight}>
          <Skeleton className={styles.skeletonUser} />
        </div>
        <div className={styles.skeletonRowLeft}>
          <Skeleton className={styles.skeletonAssistantLg} />
        </div>
        <div className={styles.skeletonRowRight}>
          <Skeleton className={styles.skeletonUserSm} />
        </div>
        <div className={styles.skeletonRowLeft}>
          <Skeleton className={styles.skeletonAssistantMd} />
        </div>
      </div>
    );
  }

  if (messages.length === 0) {
    return (
      <div className={styles.empty}>
        <p className={styles.emptyTitle}>开始和 Avalon 对话</p>
        <p className={styles.emptyHint}>输入消息，与你的智能助手交流。</p>
      </div>
    );
  }

  return (
    <ScrollArea className={styles.listRoot} viewportClassName={styles.list}>
      {messages.map((m) => (
        <MessageBubble key={m.id} message={m} />
      ))}
      <div ref={endRef} />
    </ScrollArea>
  );
}
