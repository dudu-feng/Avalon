import { useEffect, useRef } from 'react'
import type { ChatMessage } from '../../types'
import { MessageBubble } from './MessageBubble'

interface ChatMessagesProps {
  messages: ChatMessage[]
  isTyping?: boolean
}

export function ChatMessages({ messages, isTyping }: ChatMessagesProps) {
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, isTyping])

  if (messages.length === 0) {
    return (
      <div className="chat-messages empty">
        <div className="empty-state">
          <div className="empty-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M12 3L14.5 8.5L20 11L14.5 13.5L12 19L9.5 13.5L4 11L9.5 8.5L12 3Z" />
            </svg>
          </div>
          <h3>开始新对话</h3>
          <p>输入问题，与 Avalon 智能助手交流。</p>
        </div>
      </div>
    )
  }

  return (
    <div className="chat-messages">
      {messages.map((message) => (
        <MessageBubble key={message.id} message={message} />
      ))}
      {isTyping && (
        <div className="message-row agent typing">
          <div className="message-avatar" aria-hidden="true">
            AI
          </div>
          <div className="message-body">
            <div className="typing-indicator">
              <span />
              <span />
              <span />
            </div>
          </div>
        </div>
      )}
      <div ref={bottomRef} />
    </div>
  )
}
