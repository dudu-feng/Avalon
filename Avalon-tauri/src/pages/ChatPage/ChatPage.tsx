import { useEffect, useState } from 'react';
import { useChat, MessageList, ChatInput } from '../../components/features/chat';
import { Button, CircleProgress } from '../../components/ui';
import { getConfig, setActiveModel } from '../../lib/settingsApi';
import type { ModelConfig } from '../../types/config';
import styles from './ChatPage.module.css';

export function ChatPage() {
  const { messages, isBusy, send, newSession, stop, contextUsage } = useChat();
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
      <div className={styles.toolbar}>
        {contextUsage && (
          <div className={styles.usage}>
            <CircleProgress
              value={contextUsage.used_tokens}
              max={contextUsage.threshold}
              size={40}
              strokeWidth={5}
              title={`上下文 ${contextUsage.used_tokens} / ${contextUsage.threshold} tokens`}
            />
            <span className={styles.usageLabel}>上下文</span>
          </div>
        )}
        <Button variant="ghost" size="sm" onClick={newSession} disabled={isBusy}>
          ⊕ 新会话
        </Button>
      </div>
      <MessageList messages={messages} />
      <ChatInput
        onSubmit={send}
        onStop={stop}
        isBusy={isBusy}
        models={models}
        activeModel={activeModel}
        onModelChange={onModelChange}
      />
    </div>
  );
}
