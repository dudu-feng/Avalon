import type { ChatSession } from '../../types'

interface SidebarSessionListProps {
  sessions: ChatSession[]
  activeSessionId?: string
  onSelect: (id: string) => void
}

function formatTime(ts: number): string {
  const date = new Date(ts)
  const now = new Date()
  const isToday =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()

  if (isToday) {
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  }
  return date.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })
}

export function SidebarSessionList({
  sessions,
  activeSessionId,
  onSelect,
}: SidebarSessionListProps) {
  if (sessions.length === 0) {
    return <div className="session-list-empty">暂无会话</div>
  }

  return (
    <div className="session-list" role="list" aria-label="会话列表">
      {sessions.map((session) => (
        <button
          key={session.id}
          type="button"
          role="listitem"
          className={`session-item ${session.id === activeSessionId ? 'active' : ''}`}
          onClick={() => onSelect(session.id)}
        >
          <div className="session-title-row">
            <span className="session-title">{session.title}</span>
            <span className="session-time">{formatTime(session.updatedAt)}</span>
          </div>
          {session.lastMessage && (
            <span className="session-preview">{session.lastMessage}</span>
          )}
        </button>
      ))}
    </div>
  )
}
