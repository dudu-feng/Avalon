import type { ReactNode } from 'react'
import { Sidebar } from './Sidebar'
import type { ChatSession } from '../../types'

interface MainLayoutProps {
  sessions: ChatSession[]
  activeSessionId?: string
  onCreateSession: () => void
  onSelectSession: (id: string) => void
  children: ReactNode
}

export function MainLayout({
  sessions,
  activeSessionId,
  onCreateSession,
  onSelectSession,
  children,
}: MainLayoutProps) {
  return (
    <div className="app-layout">
      <Sidebar
        sessions={sessions}
        activeSessionId={activeSessionId}
        onCreateSession={onCreateSession}
        onSelectSession={onSelectSession}
      />
      <main className="main-content">{children}</main>
    </div>
  )
}
