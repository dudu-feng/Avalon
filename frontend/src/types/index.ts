export type MessageRole = 'user' | 'agent' | 'system'

export interface ChatMessage {
  id: string
  role: MessageRole
  content: string
  timestamp: number
  status?: 'sending' | 'sent' | 'error'
}

export interface ChatSession {
  id: string
  title: string
  lastMessage?: string
  updatedAt: number
  isActive?: boolean
}

export interface NavItem {
  id: string
  label: string
  icon: string
  href?: string
}
