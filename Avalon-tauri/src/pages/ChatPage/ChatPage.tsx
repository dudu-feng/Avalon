import { useChat, MessageList, ChatInput } from '../../components/features/chat';
import { Button } from '../../components/ui';
import styles from './ChatPage.module.css';

export function ChatPage() {
  const { messages, isBusy, send, newSession } = useChat();

  return (
    <div className={styles.chat}>
      <div className={styles.toolbar}>
        <Button variant="ghost" size="sm" onClick={newSession} disabled={isBusy}>
          ⊕ 新会话
        </Button>
      </div>
      <MessageList messages={messages} />
      <ChatInput onSubmit={send} disabled={isBusy} />
    </div>
  );
}
