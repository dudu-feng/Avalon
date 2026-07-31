import type { ChatSession, NavItem } from '../../types'
import { SidebarNav } from './SidebarNav'
import { SidebarSessionList } from './SidebarSessionList'

const navItems: NavItem[] = [
  { id: 'chat', label: '对话', icon: 'chat' },
  { id: 'agents', label: '智能体', icon: 'robot' },
  { id: 'memory', label: '记忆', icon: 'memory' },
  { id: 'settings', label: '设置', icon: 'settings' },
]

interface SidebarProps {
  sessions: ChatSession[]
  activeSessionId?: string
  onCreateSession: () => void
  onSelectSession: (id: string) => void
}

export function Sidebar({
  sessions,
  activeSessionId,
  onCreateSession,
  onSelectSession,
}: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <span className="brand-dot" aria-hidden="true" />
        <span className="brand-name">Avalon</span>
      </div>

      <button type="button" className="new-chat-button" onClick={onCreateSession}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <line x1="12" y1="5" x2="12" y2="19" />
          <line x1="5" y1="12" x2="19" y2="12" />
        </svg>
        新建对话
      </button>

      <SidebarSessionList
        sessions={sessions}
        activeSessionId={activeSessionId}
        onSelect={onSelectSession}
      />

      <div className="sidebar-divider" />

      <SidebarNav items={navItems} activeId="chat" />
    </aside>
  )
}
