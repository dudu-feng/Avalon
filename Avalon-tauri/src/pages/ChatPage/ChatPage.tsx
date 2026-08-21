import { useEffect, useState } from 'react';
import { useChat, MessageList, ChatInput, SessionList } from '../../components/features/chat';
import { Button, Drawer } from '../../components/ui';
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
    hasEarlier,
    loadingEarlier,
    loadEarlier,
  } = useChat();
  const [models, setModels] = useState<ModelConfig[]>([]);
  const [activeModel, setActiveModelName] = useState('');
  const [drawerOpen, setDrawerOpen] = useState(false);

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

  // 切换会话：切过去后收起抽屉（会话历史非模态，选中即隐）
  const onSelectSession = (id: string) => {
    switchSession(id);
    setDrawerOpen(false);
  };

  return (
    <div className={styles.chat}>
      <div className={styles.conversation}>
        <div className={styles.toolbar}>
          <button
            type="button"
            className={styles.menuToggle}
            onClick={() => setDrawerOpen(true)}
            aria-label="打开会话历史"
            title="会话历史"
          >
            ☰ 会话
          </button>
        </div>
        <MessageList
          messages={messages}
          loading={loading}
          resetKey={activeId}
          hasEarlier={hasEarlier}
          loadingEarlier={loadingEarlier}
          onLoadEarlier={loadEarlier}
        />
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

      <Drawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        title="会话"
        actions={
          <Button variant="ghost" size="sm" onClick={newSession} title="新建会话">
            ＋ 新建
          </Button>
        }
      >
        <SessionList
          sessions={sessions}
          activeId={activeId}
          loading={sessionsLoading}
          onSelect={onSelectSession}
          onRename={renameSession}
          onDelete={deleteSession}
        />
      </Drawer>
    </div>
  );
}
