// 聊天状态机的对外 hook —— 状态本体在 useChatStore（模块级单例）。
//
// 状态按更新频率拆成三个切片，组件只订阅自己需要的部分：
// - messages：已提交消息（低频，轮次边界 / 工具事件才变）
// - streaming：正在生成的正文（高频，每个增量都变）
// - ui：忙碌 / 会话列表 / 用量等（低频）
//
// useSyncExternalStore 在每次通知时都会读快照，但只有引用变化才触发重渲染：
// 增量更新只换 streaming 引用，订阅 messages / ui 的组件直接跳过。
// 组件里不持有任何对话状态，所以切页卸载不会丢失正在流式累积的输出。

import { useEffect, useMemo, useSyncExternalStore } from 'react';
import type { ChatMessage, MessageAttachment } from '../../../types/chat';
import * as chatStore from './useChatStore';
import type { ChatUiState, StreamState } from './useChatStore';

/** 已提交消息列表（低频） */
export function useChatMessages(): ChatMessage[] {
  const store = chatStore.getChatStore();
  return useSyncExternalStore(store.subscribe, store.getMessagesSnapshot);
}

/** 正在流式生成的正文（高频）。只应由流式气泡订阅。 */
export function useChatStreaming(): StreamState | null {
  const store = chatStore.getChatStore();
  return useSyncExternalStore(store.subscribe, store.getStreamingSnapshot);
}

/** 忙碌 / 会话列表 / 用量等 UI 状态（低频），对象引用只在相关字段变化时更新 */
export function useChatUi(): ChatUiState {
  const store = chatStore.getChatStore();
  return useSyncExternalStore(store.subscribe, store.getUiSnapshot);
}

/**
 * 动作与生命周期：返回稳定引用，不订阅任何状态。
 * 顺带承担一次性初始化（拉历史、订阅后台事件、刷新会话列表与用量），
 * 因此页面里调用一次即可，不会因为流式增量重复执行。
 */
export function useChatActions() {
  const store = chatStore.getChatStore();

  useEffect(() => {
    chatStore.ensureInit(store); // 一次性：历史只拉一次，否则每次回页都会冲掉正在累积的流式内容
    chatStore.initSessionEvents(); // 后台来源结束后回填时间线（幂等，全局只订阅一次）
    void chatStore.refreshSessions(store); // 每次挂载都刷：列表/用量是「轻」状态，不覆盖 messages
    void chatStore.refreshUsage(store);
  }, [store]);

  return useMemo(
    () => ({
      send: (text: string, attachments?: MessageAttachment[]) =>
        chatStore.send(store, text, attachments),
      newSession: () => chatStore.newSession(store),
      stop: () => chatStore.stop(store),
      compressNow: () => chatStore.compressNow(store),
      switchSession: (id: string) => chatStore.switchSession(store, id),
      deleteSession: (id: string) => chatStore.deleteSession(store, id),
      renameSession: (id: string, title: string) =>
        chatStore.renameSession(store, id, title),
      loadEarlier: () => chatStore.loadEarlier(store),
    }),
    [store],
  );
}
