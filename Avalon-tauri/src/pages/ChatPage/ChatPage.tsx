import { useEffect, useState } from 'react';
import { useChat, MessageList, ChatInput, SessionList } from '../../components/features/chat';
import { getConfig, setActiveModel } from '../../lib/settingsApi';
import type { ModelConfig } from '../../types/config';
import styles from './ChatPage.module.css';

export function ChatPage() {
  const {
    messages,
    isBusy,
    send,
    newSession,
    stop,
    contextUsage,
    loading,
    sessions,
    sessionsLoading,
    activeId,
    switchSession,
    deleteSession,
    renameSession,
  } = useChat();
  const [models, setModels] = useState<ModelConfig[]>([]);
  const [activeModel, setActiveModelName] = useState('');

  // 挂载时加载模型列表 + 当前活跃模型
  useEffect(() => {
    getConfig()
      .then((cfg) => {
        setModels(cfg.models);
        setActiveModelName(cfg.active_model);
      })
      .catch((e) => console.error('get_config 失败:', e));
  }, []);

  // 切换模型：乐观更新 UI，写回失败时恢复为后端当前值
  const onModelChange = (name: string) => {
    setActiveModelName(name);
    setActiveModel(name).catch((e) => {
      console.error('set_active_model 失败:', e);
      getConfig()
        .then((cfg) => setActiveModelName(cfg.active_model))
        .catch(() => {});
    });
  };

  return (
    <div className={styles.chat}>
      <SessionList
        sessions={sessions}
        activeId={activeId}
        loading={sessionsLoading}
        onSelect={switchSession}
        onNew={newSession}
        onRename={renameSession}
        onDelete={deleteSession}
      />
      <div className={styles.conversation}>
        <MessageList messages={messages} loading={loading} resetKey={activeId} />
        <ChatInput
          onSubmit={send}
          onStop={stop}
          isBusy={isBusy}
          models={models}
          activeModel={activeModel}
          onModelChange={onModelChange}
          contextUsage={contextUsage}
        />
      </div>
    </div>
  );
}
