import type { ChatMessage } from '../../types'

interface MessageBubbleProps {
  message: ChatMessage
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === 'user'

  return (
    <div className={`message-row ${isUser ? 'user' : 'agent'}`}>
      <div className="message-avatar" aria-hidden="true">
        {isUser ? '我' : 'AI'}
      </div>
      <div className="message-body">
        <div className="message-content">{message.content}</div>
        <div className="message-meta">
          <span>{formatTime(message.timestamp)}</span>
          {message.status === 'sending' && <span className="message-status">发送中…</span>}
          {message.status === 'error' && <span className="message-status error">发送失败</span>}
        </div>
      </div>
    </div>
  )
}
