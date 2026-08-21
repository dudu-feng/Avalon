import type { ChatMessage } from '../../../types/chat';
import { ThinkingBlock } from './ThinkingBlock';
import { ActionStepItem } from './ActionStepItem';
import { MarkdownContent } from './MarkdownContent';
import styles from './MessageBubble.module.css';

export interface MessageBubbleProps {
  message: ChatMessage;
}

export function MessageBubble({ message }: MessageBubbleProps) {
  // tool 消息：独立折叠卡片（左对齐，不套气泡外壳，参数/结果自包含）
  if (message.role === 'tool') {
    return (
      <div className={`${styles.row} ${styles.assistant}`}>
        <div className={styles.tool}>
          <ActionStepItem tool={message.tool} />
        </div>
      </div>
    );
  }

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
          (message.content || isStreaming) && (
            <MarkdownContent streaming={isStreaming}>{message.content}</MarkdownContent>
          )
        )}
      </div>
    </div>
  );
}
