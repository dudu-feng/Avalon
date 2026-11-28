// 对话 store 的类型与 UI 切片定义。

import type {
  ChatMessage,
  ContextUsage,
  MessageSource,
  SessionMeta,
} from '../../../../types/chat';

/** 正在生成的 assistant 内容。与已提交的 messages 分开存放：
 *  增量只替换这个对象，消息列表 / 工具栏 / 输入框的引用保持不变，不参与重渲染。 */
export interface StreamState {
  /** 与提交后的消息 id 一致，done / error 时直接沿用 */
  id: string;
  content: string;
  thought: string;
  source: MessageSource;
}

export interface ChatState {
  /** 已提交消息（低频）：只在轮次边界 / 工具事件 / 压缩提示时变化 */
  messages: ChatMessage[];
  /** 正在流式生成的内容（高频）：每次增量只替换它 */
  streaming: StreamState | null;
  isBusy: boolean;
  compressing: boolean;
  contextUsage: ContextUsage | null;
  /** 初始历史加载中（骨架屏）：未初始化完成前不能当作「空会话」，否则会闪一下空状态 */
  initialLoading: boolean;
  resetting: boolean;
  switching: boolean;
  sessions: SessionMeta[];
  sessionsLoading: boolean;
  /** 渐进式加载历史游标：当前已加载的最早块号 / 是否还有更早块 / 是否正在加载更早 */
  earliestChunk: number | null;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  /** 当前活跃会话 id：由列表推导（后端保证 active 唯一且置顶），随 setSessions 更新 */
  activeId: string;
  /** 正在实时渲染的后台来源（桌面自己的轮次用 isBusy 表示） */
  activeSource: MessageSource | null;
}

/** UI 切片字段：这些字段低频变化，合并成一个稳定对象供 useChatUi 订阅 */
export const UI_KEYS = [
  'isBusy',
  'compressing',
  'contextUsage',
  'initialLoading',
  'resetting',
  'switching',
  'sessions',
  'sessionsLoading',
  'earliestChunk',
  'hasEarlier',
  'loadingEarlier',
  'activeId',
  'activeSource',
] as const;

export interface ChatUiState extends Pick<ChatState, (typeof UI_KEYS)[number]> {
  /** 是否存在正在生成的流式气泡：只随「有/无」变化重建，不随增量变化。
   *  供列表在消息为空时决定是否渲染流式气泡（不能靠气泡自己上报，否则空列表里永远不会挂载）。 */
  hasStreaming: boolean;
}

export function toUiSnapshot(state: ChatState): ChatUiState {
  return {
    isBusy: state.isBusy,
    compressing: state.compressing,
    contextUsage: state.contextUsage,
    initialLoading: state.initialLoading,
    resetting: state.resetting,
    switching: state.switching,
    sessions: state.sessions,
    sessionsLoading: state.sessionsLoading,
    earliestChunk: state.earliestChunk,
    hasEarlier: state.hasEarlier,
    loadingEarlier: state.loadingEarlier,
    activeId: state.activeId,
    activeSource: state.activeSource,
    hasStreaming: state.streaming !== null,
  };
}

export interface ChatStore {
  state: ChatState;
  /** UI 切片的缓存快照：只有 UI_KEYS 变化时才重建，增量更新不会让它换引用 */
  uiSnapshot: ChatUiState;
  /** 当前正在执行的桌面 run_id（stop 时定位；后台来源不占用这个槽位） */
  currentRunId: string;
  listeners: Set<() => void>;
  /** 同步置位（在第一个 await 之前），StrictMode 二次挂载靠它幂等 */
  initStarted: boolean;
  initDone: boolean;
  /** 「历史代次」：整体替换 messages 的动作推进它，用于作废在飞的 init 写入 */
  epoch: number;
  /** msg-* 计数器（桶内自增，不再是组件 useRef —— 卸载重挂不再从 1 重来） */
  idSeq: number;
  /** 本轮是否已请求停止：send 收尾时据此重拉后端历史，丢弃被中断的轮次 */
  stopRequested: boolean;
  subscribe: (listener: () => void) => () => void;
  getSnapshot: () => ChatState;
  getMessagesSnapshot: () => ChatMessage[];
  getStreamingSnapshot: () => StreamState | null;
  getUiSnapshot: () => ChatUiState;
}

export const INITIAL_STATE: ChatState = {
  messages: [],
  streaming: null,
  isBusy: false,
  compressing: false,
  contextUsage: null,
  initialLoading: true,
  resetting: false,
  switching: false,
  sessions: [],
  sessionsLoading: true,
  earliestChunk: null,
  hasEarlier: false,
  loadingEarlier: false,
  activeId: '',
  activeSource: null,
};
