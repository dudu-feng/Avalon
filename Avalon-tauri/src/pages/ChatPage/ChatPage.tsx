import { useChat, MessageList, ChatInput } from '../../components/features/chat';
import styles from './ChatPage.module.css';

export function ChatPage() {
  const { messages, isBusy, send } = useChat();

  return (
    <div className={styles.chat}>
      <MessageList messages={messages} />
      <ChatInput onSubmit={send} disabled={isBusy} />
    </div>
  );
}
