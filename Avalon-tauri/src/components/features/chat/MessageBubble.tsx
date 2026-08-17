import type { ChatMessage } from '../../../types/chat';
import { ThinkingBlock } from './ThinkingBlock';
import { ActionBlock } from './ActionBlock';
import styles from './MessageBubble.module.css';

export interface MessageBubbleProps {
  message: ChatMessage;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === 'user';
  const isError = message.status === 'error';
  const isStreaming = message.status === 'streaming';

  const rowClass = [styles.row, isUser ? styles.user : styles.assistant, isError && styles.error]
    .filter(Boolean)
    .join(' ');

  if (isUser) {
    return (
      <div className={rowClass}>
        <div className={styles.bubble}>
          <p className={styles.text}>{message.content}</p>
        </div>
      </div>
    );
  }

  return (
    <div className={rowClass}>
      <div className={styles.bubble}>
        {message.thought && <ThinkingBlock thought={message.thought} />}
        {isError ? (
          <p className={styles.text}>{message.error}</p>
        ) : (
          <p className={styles.text}>
            {message.content}
            {isStreaming && <span className={styles.cursor}>▌</span>}
          </p>
        )}
        {message.actions.length > 0 && (
          <div className={styles.actions}>
            {message.actions.map((block, i) => (
              <ActionBlock key={i} block={block} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
