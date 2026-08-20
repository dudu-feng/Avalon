import { useChat, MessageList, ChatInput } from '../../components/features/chat';
import { Button, CircleProgress } from '../../components/ui';
import styles from './ChatPage.module.css';

export function ChatPage() {
  const { messages, isBusy, send, newSession, contextUsage } = useChat();

  return (
    <div className={styles.chat}>
      <div className={styles.toolbar}>
        {contextUsage && (
          <div className={styles.usage}>
            <CircleProgress
              value={contextUsage.used_tokens}
              max={contextUsage.threshold}
              size={40}
              strokeWidth={5}
              title={`上下文 ${contextUsage.used_tokens} / ${contextUsage.threshold} tokens`}
            />
            <span className={styles.usageLabel}>上下文</span>
          </div>
        )}
        <Button variant="ghost" size="sm" onClick={newSession} disabled={isBusy}>
          ⊕ 新会话
        </Button>
      </div>
      <MessageList messages={messages} />
      <ChatInput onSubmit={send} disabled={isBusy} />
    </div>
  );
}
