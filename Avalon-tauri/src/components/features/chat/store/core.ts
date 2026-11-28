// store 实例与唯一写入口：整对象替换、UI 切片缓存、消息 id 与 session 列表。

import type { SessionMeta } from '../../../../types/chat';
import {
  INITIAL_STATE,
  UI_KEYS,
  toUiSnapshot,
  type ChatState,
  type ChatStore,
} from './types';

export function createStore(): ChatStore {
  const listeners = new Set<() => void>();
  const store: ChatStore = {
    state: INITIAL_STATE,
    uiSnapshot: toUiSnapshot(INITIAL_STATE),
    currentRunId: '',
    listeners,
    initStarted: false,
    initDone: false,
    epoch: 0,
    idSeq: 0,
    stopRequested: false,
    // subscribe / getSnapshot 在创建时固化为稳定引用：useSyncExternalStore 要求每次渲染
    // 拿到同一个函数，否则每次渲染都会重新订阅。
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot() {
      return store.state;
    },
    getMessagesSnapshot() {
      return store.state.messages;
    },
    getStreamingSnapshot() {
      return store.state.streaming;
    },
    getUiSnapshot() {
      return store.uiSnapshot;
    },
  };
  return store;
}

/** 全局唯一 store：单会话、单时间线。 */
export const store = createStore();

export function getChatStore(): ChatStore {
  return store;
}

/** 唯一写入口：整对象替换 + 通知。getSnapshot 必须在两次更新之间返回同一引用，
 *  否则 useSyncExternalStore 会认为「每次读都变了」→ 无限重渲染。 */
export function setState(
  store: ChatStore,
  patch: Partial<ChatState> | ((prev: ChatState) => Partial<ChatState>),
) {
  const prev = store.state;
  const next = typeof patch === 'function' ? patch(prev) : patch;
  const merged = { ...prev, ...next };

  // UI 切片只在相关字段变化时重建引用：增量更新只换 streaming，不会惊动 useChatUi。
  // 流式气泡的「有/无」也是 UI 关注点，但比较的是存在性而非对象引用，增量不会触发重建。
  const streamingPresenceChanged = (merged.streaming !== null) !== (prev.streaming !== null);
  if (streamingPresenceChanged || UI_KEYS.some((key) => merged[key] !== prev[key])) {
    store.uiSnapshot = toUiSnapshot(merged);
  }

  store.state = merged;
  store.listeners.forEach((l) => l());
}

/** sessions 的唯一写入口：activeId 顺带由列表推导（后端保证 active 唯一且置顶） */
export function setSessions(store: ChatStore, sessions: SessionMeta[]) {
  setState(store, {
    sessions,
    activeId: sessions.find((s) => s.status === 'active')?.id ?? '',
    sessionsLoading: false,
  });
}

/** 消息 id：桶内自增。唯一性只需在一个消息列表内成立，
 *  而列表切换时整份替换，故按桶计数足够；hist-* 前缀来自 mapHistoryMessages，与之天然不撞。 */
export function nextId(store: ChatStore): string {
  return `msg-${(store.idSeq += 1)}`;
}
