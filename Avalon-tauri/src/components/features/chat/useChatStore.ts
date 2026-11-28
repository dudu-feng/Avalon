// 对话状态 store —— 模块级单例（useSyncExternalStore）。
//
// 实现在 ./store/ 下按职责拆分；本文件保持原导出路径，避免调用方批量改动。
//
// 为什么状态不放组件里：流式事件经 chatApi 的 Channel 回调送达，回调闭包一旦属于
// 已卸载的组件，React 就会静默丢弃 —— 切页即丢整轮输出。状态提到模块作用域后，
// 页面卸载不影响事件累积，切回来直接渲染。
//
// 统一会话后全局只有一个 main 会话，因此只有一个桶；飞书 / QQ / 定时任务的
// 后台轮次通过 session-updated 事件在结束后回填，不再按渠道各建一份状态。

export { createStore, getChatStore } from './store/core';
export { applyEvent } from './store/reducer';
export type { ChatState, ChatStore, ChatUiState, StreamState } from './store/types';
export {
  ensureInit,
  flushStaleRefresh,
  initSessionEvents,
  refreshSessions,
  refreshUsage,
} from './store/lifecycle';
export {
  compressNow,
  deleteSession,
  loadEarlier,
  loadTaskMessages,
  newSession,
  renameSession,
  send,
  stop,
  switchSession,
} from './store/actions';
