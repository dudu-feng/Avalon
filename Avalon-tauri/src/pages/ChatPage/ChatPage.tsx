import { useEffect, useMemo, useState } from 'react';
import {
  useChatActions,
  useChatMessages,
  useChatUi,
  MessageList,
  ChatInput,
  SessionList,
} from '../../components/features/chat';
import { Button, Drawer } from '../../components/ui';
import { visionStatus } from '../../lib/fileApi';
import { getConfig, setActiveModel } from '../../lib/settingsApi';
import type { ChatMessage, MessageSource } from '../../types/chat';
import type { ModelConfig } from '../../types/config';
import styles from './ChatPage.module.css';

type SourceFilter = 'all' | MessageSource;

const SOURCE_NAMES: Record<MessageSource, string> = {
  desktop: '桌面',
  feishu: '飞书',
  qq: 'QQ',
  task: '定时任务',
};

const FILTERS: [SourceFilter, string][] = [
  ['all', '全部'],
  ['desktop', '桌面'],
  ['feishu', '飞书'],
  ['qq', 'QQ'],
  ['task', '定时任务'],
];

/** 按来源筛选时间线：以 user 消息为组边界，assistant / tool 跟随所在组。
 *  实时流式消息没有 origin 时沿用上一组的来源，避免筛选后凭空消失。 */
function filterBySource(messages: ChatMessage[], filter: SourceFilter): ChatMessage[] {
  if (filter === 'all') return messages;
  const out: ChatMessage[] = [];
  let current: MessageSource = 'desktop';
  for (const m of messages) {
    const origin = 'origin' in m ? m.origin : undefined;
    if (m.role === 'user') {
      current = origin?.source ?? 'desktop';
    } else if (origin?.source) {
      current = origin.source;
    }
    if (current === filter) out.push(m);
  }
  return out;
}

export function ChatPage() {
  // 分片订阅：messages / ui 低频，流式增量不会让整页重渲染（由 MessageList 内部的
  // StreamingBubble 单独订阅）；动作引用稳定，不随状态变化。
  const messages = useChatMessages();
  const ui = useChatUi();
  const {
    send,
    newSession,
    stop,
    compressNow,
    switchSession,
    deleteSession,
    renameSession,
    loadEarlier,
  } = useChatActions();
  const {
    isBusy,
    compressing,
    contextUsage,
    initialLoading,
    resetting,
    switching,
    sessions,
    sessionsLoading,
    activeId,
    hasEarlier,
    loadingEarlier,
    activeSource,
    hasStreaming,
  } = ui;
  const loading = initialLoading || resetting || switching;
  // 有执行中 / 排队中的轮次时禁止会话控制操作（后端同样会拒绝）
  const sessionBusy = isBusy || activeSource !== null;
  const [models, setModels] = useState<ModelConfig[]>([]);
  const [activeModel, setActiveModelName] = useState('');
  const [visionEnabled, setVisionEnabled] = useState(false);
  const [visionModel, setVisionModel] = useState('');
  const [confirmBeforeSend, setConfirmBeforeSend] = useState(true);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>('all');
  const visibleMessages = useMemo(
    () => filterBySource(messages, sourceFilter),
    [messages, sourceFilter],
  );
  // 流式气泡按同一来源筛选规则显示：桌面轮次用 isBusy 表示，后台轮次用 activeSource
  const liveSource: MessageSource | null = isBusy ? 'desktop' : activeSource;
  const streamingVisible =
    sourceFilter === 'all' || liveSource === null || sourceFilter === liveSource;
  // 列表空态 / 滚动跟随需要知道「是否有一个可见的流式气泡」。
  // 由 store 的存在性（低频布尔）+ 来源筛选推导，不能等气泡自己上报 —— 空列表里它压根不会挂载。
  const streamPresent = hasStreaming && streamingVisible;

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
          <div className={styles.filters}>
            {FILTERS.map(([key, label]) => (
              <button
                key={key}
                type="button"
                className={`${styles.filterChip} ${sourceFilter === key ? styles.filterChipActive : ''}`}
                onClick={() => setSourceFilter(key)}
              >
                {label}
              </button>
            ))}
          </div>
          {(isBusy || activeSource) && (
            <span className={styles.runStatus}>
              {isBusy
                ? '正在处理桌面消息…'
                : `正在处理来自${SOURCE_NAMES[activeSource as MessageSource]}的请求…`}
            </span>
          )}
        </div>
        <MessageList
          messages={visibleMessages}
          loading={loading}
          resetKey={activeId}
          hasEarlier={hasEarlier}
          loadingEarlier={loadingEarlier}
          onLoadEarlier={loadEarlier}
          withStreaming
          streamingVisible={streamingVisible}
          streamPresent={streamPresent}
        />
        {sourceFilter === 'all' || sourceFilter === 'desktop' || isBusy ? (
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
        ) : (
          <div className={styles.readonlyHint}>
            仅查看{SOURCE_NAMES[sourceFilter]}历史消息；切回「全部」或「桌面」后可继续对话
          </div>
        )}
      </div>

      <Drawer
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        title="会话"
        actions={
          <Button
            variant="ghost"
            size="sm"
            onClick={newSession}
            title={sessionBusy ? '有消息正在执行或排队，暂不能新建会话' : '新建会话'}
            disabled={sessionBusy}
          >
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
