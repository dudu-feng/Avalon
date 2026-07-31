interface ChatHeaderProps {
  title?: string
  subtitle?: string
}

export function ChatHeader({ title = '对话', subtitle }: ChatHeaderProps) {
  return (
    <header className="chat-header">
      <div>
        <h1>{title}</h1>
        {subtitle && <p>{subtitle}</p>}
      </div>
      <div className="chat-header-actions">
        <button type="button" className="icon-button" aria-label="更多选项">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="1" />
            <circle cx="19" cy="12" r="1" />
            <circle cx="5" cy="12" r="1" />
          </svg>
        </button>
      </div>
    </header>
  )
}
