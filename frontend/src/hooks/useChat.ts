import { useState, useCallback } from 'react'
import type { ChatMessage, ChatSession } from '../types'

let idCounter = 0
const makeId = (prefix: string) => `${prefix}-${Date.now()}-${idCounter++}`

const initialSessions: ChatSession[] = [
  {
    id: 'session-1',
    title: '新对话',
    lastMessage: '你好，我是 Avalon 智能助手。',
    updatedAt: Date.now(),
    isActive: true,
  },
]

const initialMessages: ChatMessage[] = [
  {
    id: makeId('msg'),
    role: 'agent',
    content: '你好，我是 Avalon 智能助手。有什么可以帮你的吗？',
    timestamp: Date.now(),
  },
]

export function useChat() {
  const [sessions, setSessions] = useState<ChatSession[]>(initialSessions)
  const [messages, setMessages] = useState<ChatMessage[]>(initialMessages)
  const [inputValue, setInputValue] = useState('')
  const [isTyping, setIsTyping] = useState(false)

  const activeSession = sessions.find((s) => s.isActive) ?? sessions[0]

  const createSession = useCallback(() => {
    const newSession: ChatSession = {
      id: makeId('session'),
      title: '新对话',
      updatedAt: Date.now(),
      isActive: true,
    }
    setSessions((prev) => [...prev.map((s) => ({ ...s, isActive: false })), newSession])
    setMessages([])
  }, [])

  const selectSession = useCallback((id: string) => {
    setSessions((prev) =>
      prev.map((s) => ({ ...s, isActive: s.id === id }))
    )
    // 实际项目中这里应加载对应会话的历史消息
    setMessages((prev) =>
      prev.length === 0
        ? [
            {
              id: makeId('msg'),
              role: 'agent',
              content: '已切换会话，继续聊吧。',
              timestamp: Date.now(),
            },
          ]
        : prev
    )
  }, [])

  const sendMessage = useCallback(
    async (content: string) => {
      if (!content.trim()) return

      const userMessage: ChatMessage = {
        id: makeId('msg'),
        role: 'user',
        content: content.trim(),
        timestamp: Date.now(),
        status: 'sending',
      }

      setMessages((prev) => [...prev, userMessage])
      setInputValue('')
      setIsTyping(true)

      // TODO: 接入后端 API
      await new Promise((resolve) => setTimeout(resolve, 800))

      const agentMessage: ChatMessage = {
        id: makeId('msg'),
        role: 'agent',
        content: `收到你的消息："${content.trim()}"`,
        timestamp: Date.now(),
        status: 'sent',
      }

      setMessages((prev) =>
        prev
          .map((m) => (m.id === userMessage.id ? { ...m, status: 'sent' as const } : m))
          .concat(agentMessage)
      )
      setSessions((prev) =>
        prev.map((s) =>
          s.id === activeSession.id
            ? { ...s, lastMessage: content.trim(), updatedAt: Date.now() }
            : s
        )
      )
      setIsTyping(false)
    },
    [activeSession.id]
  )

  return {
    sessions,
    messages,
    inputValue,
    setInputValue,
    isTyping,
    activeSession,
    createSession,
    selectSession,
    sendMessage,
  }
}
