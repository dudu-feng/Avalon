import { MainLayout } from '../components/layout/MainLayout'
import { ChatHeader } from '../components/chat/ChatHeader'
import { ChatMessages } from '../components/chat/ChatMessages'
import { ChatInput } from '../components/chat/ChatInput'
import { useChat } from '../hooks/useChat'

export function ChatPage() {
  const {
    sessions,
    messages,
    inputValue,
    setInputValue,
    isTyping,
    activeSession,
    createSession,
    selectSession,
    sendMessage,
  } = useChat()

  return (
    <MainLayout
      sessions={sessions}
      activeSessionId={activeSession?.id}
      onCreateSession={createSession}
      onSelectSession={selectSession}
    >
      <div className="chat-page">
        <ChatHeader title={activeSession?.title ?? '对话'} />
        <ChatMessages messages={messages} isTyping={isTyping} />
        <ChatInput
          value={inputValue}
          onChange={setInputValue}
          onSubmit={sendMessage}
          disabled={isTyping}
        />
      </div>
    </MainLayout>
  )
}
