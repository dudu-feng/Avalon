import type { ChatMessage } from '../../../types/chat';
import { ThinkingBlock } from './ThinkingBlock';
import styles from './MessageBubble.module.css';

export interface MessageBubbleProps {
  message: ChatMessage;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === 'user';
  const isPending = message.status === 'pending';
  const isError = message.status === 'error';

  const rowClass = [styles.row, isUser ? styles.user : styles.assistant, isError && styles.error]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={rowClass}>
      <div className={styles.bubble}>
        {message.thought && <ThinkingBlock thought={message.thought} />}
        {isPending ? (
          <span className={styles.pending}>思考中…</span>
        ) : (
          <p className={styles.text}>{isError ? message.error : message.content}</p>
        )}
      </div>
    </div>
  );
}
