import { useEffect, useState } from 'react';
import { useChat, MessageList, ChatInput, SessionList } from '../../components/features/chat';
import { Button, Drawer } from '../../components/ui';
import { visionStatus } from '../../lib/fileApi';
import { getConfig, setActiveModel } from '../../lib/settingsApi';
import type { ModelConfig } from '../../types/config';
import styles from './ChatPage.module.css';

export function ChatPage() {
  const {
    messages,
    isBusy,
    compressing,
    send,
    newSession,
    stop,
    compressNow,
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
  const [visionEnabled, setVisionEnabled] = useState(false);
  const [visionModel, setVisionModel] = useState('');
  const [confirmBeforeSend, setConfirmBeforeSend] = useState(true);
  const [drawerOpen, setDrawerOpen] = useState(false);

  // 模型能力开了、但视觉总开关没开：输入框给出提示，避免以为图片入口坏了
  const activeSupportsVision = models.some(
    (m) => m.name === activeModel && m.supports_vision,
  );

  // 视觉能力随活跃模型变化：切模型后重新问一次后端
  const refreshVision = () => {
    visionStatus()
      .then((s) => {
        setVisionEnabled(s.enabled);
        setVisionModel(s.model ?? '');
      })
      .catch((e) => console.error('vision_status 失败:', e));
  };

  // 挂载时加载模型列表 + 当前活跃模型
  useEffect(() => {
    getConfig()
      .then((cfg) => {
        setModels(cfg.models);
        setActiveModelName(cfg.active_model);
        setConfirmBeforeSend(cfg.vision.confirm_before_send !== false);
      })
      .catch((e) => console.error('get_config 失败:', e));
    refreshVision();
  }, []);

  // 切换模型：乐观更新 UI，写回失败时恢复为后端当前值
  const onModelChange = (name: string) => {
    setActiveModelName(name);
    setActiveModel(name)
      .then(refreshVision)
      .catch((e) => {
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
          onCompress={compressNow}
          compressing={compressing}
          visionEnabled={visionEnabled}
          visionCapableButDisabled={!visionEnabled && activeSupportsVision}
          visionModel={models.find((m) => m.name === visionModel)?.modelname || visionModel}
          confirmBeforeSend={confirmBeforeSend}
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
