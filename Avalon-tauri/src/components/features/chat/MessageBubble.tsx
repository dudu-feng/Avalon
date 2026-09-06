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

  // compress 消息：压缩上下文的过程提示条（居中，不套气泡外壳，不入历史）
  if (message.role === 'compress') {
    const label =
      message.status === 'running'
        ? '正在压缩上下文…'
        : message.status === 'done'
          ? '上下文已压缩'
          : `压缩上下文失败${message.error ? `：${message.error}` : ''}`;
    return (
      <div className={styles.compressRow}>
        {message.status === 'running' ? (
          <span className={styles.compressSpinner} />
        ) : (
          <span className={styles.compressIcon}>{message.status === 'done' ? '✓' : '✗'}</span>
        )}
        <span className={styles.compressLabel} data-status={message.status}>
          {label}
        </span>
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
