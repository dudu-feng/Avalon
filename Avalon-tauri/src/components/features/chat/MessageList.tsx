import { useEffect, useRef } from 'react';
import type { ChatMessage } from '../../../types/chat';
import { MessageBubble } from './MessageBubble';
import styles from './MessageList.module.css';

export interface MessageListProps {
  messages: ChatMessage[];
}

export function MessageList({ messages }: MessageListProps) {
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  if (messages.length === 0) {
    return (
      <div className={styles.empty}>
        <p className={styles.emptyTitle}>开始和 Avalon 对话</p>
        <p className={styles.emptyHint}>输入消息，与你的智能助手交流。</p>
      </div>
    );
  }

  return (
    <div className={styles.list}>
      {messages.map((m) => (
        <MessageBubble key={m.id} message={m} />
      ))}
      <div ref={endRef} />
    </div>
  );
}
