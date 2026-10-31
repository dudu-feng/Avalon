// 聊天状态机的对外 hook —— 状态本体在 useChatStore（模块级分桶单例）。
//
// 这里只做三件事：取桶、订阅、把动作包成稳定引用。组件里不持有任何对话状态，
// 所以切页卸载不会丢失正在流式累积的输出。

import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';
import { DEFAULT_CHANNEL } from '../../../lib/chatApi';
import type { MessageAttachment } from '../../../types/chat';
import * as chatStore from './useChatStore';

export interface UseChatOptions {
  channelName?: string;
}

export function useChat(options: UseChatOptions = {}) {
  const { channelName = DEFAULT_CHANNEL } = options;

  // useMemo 固化桶引用：同一次挂载内 getSnapshot 与 subscribe 必须来自同一个 store 对象，
  // 否则会出现「读的是新桶、订阅的是旧桶」→ 永不重渲染。
  const store = useMemo(() => chatStore.getChatStore(channelName), [channelName]);
  const s = useSyncExternalStore(store.subscribe, store.getSnapshot);

  useEffect(() => {
    chatStore.ensureInit(store); // 一次性：历史只拉一次，否则每次回页都会冲掉正在累积的流式内容
    void chatStore.refreshSessions(store); // 每次挂载都刷：列表/用量是「轻」状态，不覆盖 messages
    void chatStore.refreshUsage(store);
  }, [store]);

  // 动作必须用 useCallback 固定引用：MessageList 的滚轮 effect 依赖 onLoadEarlier，
  // 内联箭头会让它每次父组件重渲染都重绑监听，甚至丢掉手势中的滚轮事件。
  return {
    messages: s.messages,
    isBusy: s.isBusy,
    compressing: s.compressing,
    contextUsage: s.contextUsage,
    loading: s.initialLoading || s.resetting || s.switching,
    resetting: s.resetting,
    switching: s.switching,
    sessions: s.sessions,
    sessionsLoading: s.sessionsLoading,
    activeId: s.activeId,
    earliestChunk: s.earliestChunk,
    hasEarlier: s.hasEarlier,
    loadingEarlier: s.loadingEarlier,
    send: useCallback(
      (text: string, attachments?: MessageAttachment[]) => chatStore.send(store, text, attachments),
      [store],
    ),
    newSession: useCallback(() => chatStore.newSession(store), [store]),
    stop: useCallback(() => chatStore.stop(store), [store]),
    compressNow: useCallback(() => chatStore.compressNow(store), [store]),
    switchSession: useCallback((id: string) => chatStore.switchSession(store, id), [store]),
    deleteSession: useCallback((id: string) => chatStore.deleteSession(store, id), [store]),
    renameSession: useCallback(
      (id: string, title: string) => chatStore.renameSession(store, id, title),
      [store],
    ),
    loadEarlier: useCallback(() => chatStore.loadEarlier(store), [store]),
  };
}
