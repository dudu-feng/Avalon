// 对话状态 store —— 模块级单例（useSyncExternalStore）。
//
// 为什么状态不放组件里：流式事件经 chatApi 的 Channel 回调送达，回调闭包一旦属于
// 已卸载的组件，React 就会静默丢弃 —— 切页即丢整轮输出。状态提到模块作用域后，
// 页面卸载不影响事件累积，切回来直接渲染。
//
// 统一会话后全局只有一个 main 会话，因此只有一个桶；飞书 / QQ / 定时任务的
// 后台轮次通过 session-updated 事件在结束后回填，不再按渠道各建一份状态。

import { listen } from '@tauri-apps/api/event';
import type {
  ChatMessage,
  ContextUsage,
  EngineEvent,
  HistoryMessage,
  MessageAttachment,
  MessageSource,
  SessionEventPayload,
  SessionMeta,
} from '../../../types/chat';
import {
  chat,
  compressSession,
  createSession,
  deleteSession as deleteSessionApi,
  getContextUsage,
  getCurrentSession,
  listSessions,
  loadSessionHistory,
  renameSession as renameSessionApi,
  stopChat,
  switchSession as switchSessionApi,
} from '../../../lib/chatApi';

type AssistantMessage = Extract<ChatMessage, { role: 'assistant' }>;
type ToolMessage = Extract<ChatMessage, { role: 'tool' }>;
type CompressMessage = Extract<ChatMessage, { role: 'compress' }>;

/** 后端历史消息（user/assistant/tool 平铺）→ 前端展示消息（逐条直映，无归并）。
 *  key 区分来源（块号 / 'cur'），保证拼接更早块时消息 id 全局唯一。 */
function mapHistoryMessages(msgs: HistoryMessage[], key: string): ChatMessage[] {
  const out: ChatMessage[] = [];
  let idx = 0;

  for (const m of msgs) {
    if (m.role === 'user') {
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'user',
        status: 'done',
        content: m.content,
        attachments: m.meta?.attachments,
        origin: m.meta ?? null,
        time: m.time,
      });
    } else if (m.role === 'assistant') {
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'assistant',
        status: 'done',
        thought: m.reasoning_content ?? '',
        content: m.content,
        tokenUsage: m.token_usage,
        origin: m.origin ?? null,
        time: m.time,
      });
    } else {
      // role === 'tool'：独立折叠卡片，参数/状态/结果自包含，不依附 assistant
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'tool',
        tool: {
          id: m.tool_call_id,
          toolName: m.name,
          arguments: m.arguments,
          status: m.success ? 'success' : 'error',
          result: m.content,
        },
        origin: m.origin ?? null,
        time: m.time,
      });
    }
  }
  return out;
}

/** 定位最后一条指定 role 的消息（assistant 事件更新末 assistant；tool_result 更新末 tool） */
function lastIndexByRole(messages: ChatMessage[], role: ChatMessage['role']): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === role) return i;
  }
  return -1;
}

/** 新建一条空的 streaming assistant 气泡（首轮由 send 预创建，后续轮由 round_start 触发） */
function newAssistant(id: string): ChatMessage {
  return { id, role: 'assistant', status: 'streaming', thought: '', content: '' };
}

/** 事件 → 新消息数组（纯函数，供消息列表函数式更新） */
function applyEvent(prev: ChatMessage[], ev: EngineEvent, newId: () => string): ChatMessage[] {
  // 轮次边界：封口上一轮 assistant、开新气泡（第一轮复用 send 预创建的空气泡）
  if (ev.type === 'round_start') {
    const idx = lastIndexByRole(prev, 'assistant');
    if (idx < 0) return prev;
    const current = prev[idx] as AssistantMessage;
    const isEmpty = !current.thought && !current.content;
    if (isEmpty) return prev;
    const copy = [...prev];
    copy[idx] = { ...current, status: 'done' };
    return [...copy, newAssistant(newId())];
  }

  // 工具发起：追加独立 tool 消息（running 状态，tool_result 再回填结果）
  if (ev.type === 'tool_call') {
    return [
      ...prev,
      {
        id: newId(),
        role: 'tool',
        tool: { id: ev.id, toolName: ev.tool_name, arguments: ev.arguments, status: 'running' },
      },
    ];
  }

  // 工具结果：更新最后一条 tool 消息的状态与结果
  if (ev.type === 'tool_result') {
    const idx = lastIndexByRole(prev, 'tool');
    if (idx < 0) return prev;
    const copy = [...prev];
    const current = copy[idx] as ToolMessage;
    copy[idx] = {
      ...current,
      tool: { ...current.tool, result: ev.result, status: ev.success ? 'success' : 'error' },
    };
    return copy;
  }

  // 压缩开始：追加独立提示气泡（running），compress_done 再回填完成/失败
  if (ev.type === 'compress_start') {
    return [...prev, { id: newId(), role: 'compress', status: 'running' }];
  }

  // 压缩结束：更新最后一条 compress 气泡的状态与错误信息
  if (ev.type === 'compress_done') {
    const idx = lastIndexByRole(prev, 'compress');
    if (idx < 0) return prev;
    const copy = [...prev];
    const current = copy[idx] as CompressMessage;
    copy[idx] = {
      ...current,
      status: ev.success ? 'done' : 'error',
      error: ev.error ?? undefined,
    };
    return copy;
  }

  // 其余事件作用于最后一条 assistant
  const idx = lastIndexByRole(prev, 'assistant');
  if (idx < 0) return prev;

  const current = prev[idx] as AssistantMessage;
  let next: AssistantMessage = current;

  switch (ev.type) {
    case 'thought_delta':
      next = { ...current, thought: current.thought + ev.delta };
      break;
    case 'message_delta':
      next = { ...current, content: current.content + ev.delta };
      break;
    case 'done':
      next = {
        ...current,
        status: 'done',
        tokenUsage: ev.result.usage,
        content: ev.result.message || current.content,
      };
      break;
    case 'error':
      next = { ...current, error: ev.message };
      break;
    default:
      return prev;
  }

  const copy = [...prev];
  copy[idx] = next;
  return copy;
}

export interface ChatState {
  messages: ChatMessage[];
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

export interface ChatStore {
  state: ChatState;
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
}

const INITIAL_STATE: ChatState = {
  messages: [],
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

function createStore(): ChatStore {
  const listeners = new Set<() => void>();
  const store: ChatStore = {
    state: INITIAL_STATE,
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
  };
  return store;
}

/** 全局唯一 store：单会话、单时间线。 */
const store = createStore();

export function getChatStore(): ChatStore {
  return store;
}

/** 唯一写入口：整对象替换 + 通知。getSnapshot 必须在两次更新之间返回同一引用，
 *  否则 useSyncExternalStore 会认为「每次读都变了」→ 无限重渲染。 */
function setState(
  store: ChatStore,
  patch: Partial<ChatState> | ((prev: ChatState) => Partial<ChatState>),
) {
  const next = typeof patch === 'function' ? patch(store.state) : patch;
  store.state = { ...store.state, ...next };
  store.listeners.forEach((l) => l());
}

/** sessions 的唯一写入口：activeId 顺带由列表推导（后端保证 active 唯一且置顶） */
function setSessions(store: ChatStore, sessions: SessionMeta[]) {
  setState(store, {
    sessions,
    activeId: sessions.find((s) => s.status === 'active')?.id ?? '',
    sessionsLoading: false,
  });
}

/** 流式高频路径专用：applyEvent 未改变数组时跳过通知（round_start 复用空气泡、未知事件会走这条） */
function updateMessages(store: ChatStore, updater: (prev: ChatMessage[]) => ChatMessage[]) {
  const next = updater(store.state.messages);
  if (next === store.state.messages) return;
  setState(store, { messages: next });
}

/** 消息 id：桶内自增。唯一性只需在一个消息列表内成立，
 *  而列表切换时整份替换，故按桶计数足够；hist-* 前缀来自 mapHistoryMessages，与之天然不撞。 */
function nextId(store: ChatStore): string {
  return `msg-${(store.idSeq += 1)}`;
}

// ============ 流式增量合并 ============
//
// 后端每产生一个 token 就发一条 message_delta。若逐条 setState，React 会为每个
// token 重渲染整棵消息列表，并对正在生成的消息反复做完整 Markdown 解析；长回答时
// 渲染速度跟不上 token 到达速度，积压成「飞机已发完、前端还在打字」。
// 这里把增量按帧级别合并（60ms 一次），渲染次数从「token 数」降到「时长 × 16」。
//
// 非增量事件（round_start / tool_call / done / error…）必须先冲刷缓冲再应用，
// 保证消息顺序与轮次边界不被合并打乱。

const STREAM_FLUSH_MS = 60;

let desktopStream: { epoch: number; events: EngineEvent[] } | null = null;
let bgStream: { runId: string; events: EngineEvent[] } | null = null;
let streamTimer: number | undefined;

/** 合并相邻同类增量：一次 setState 只做一次字符串拼接，减少中间态分配 */
function coalesceStreamEvents(events: EngineEvent[]): EngineEvent[] {
  const out: EngineEvent[] = [];
  for (const ev of events) {
    const last = out[out.length - 1];
    if (ev.type === 'message_delta' && last?.type === 'message_delta') {
      out[out.length - 1] = { type: 'message_delta', delta: last.delta + ev.delta };
    } else if (ev.type === 'thought_delta' && last?.type === 'thought_delta') {
      out[out.length - 1] = { type: 'thought_delta', delta: last.delta + ev.delta };
    } else {
      out.push(ev);
    }
  }
  return out;
}

function flushStreams() {
  if (streamTimer !== undefined) {
    window.clearTimeout(streamTimer);
    streamTimer = undefined;
  }

  const desktop = desktopStream;
  const background = bgStream;
  desktopStream = null;
  bgStream = null;

  // 会话已切换 / 用户已停止：缓冲内容属于作废轮次，直接丢弃
  if (desktop && desktop.epoch === store.epoch && !store.stopRequested) {
    const events = coalesceStreamEvents(desktop.events);
    updateMessages(store, (prev) =>
      events.reduce((acc, ev) => applyEvent(acc, ev, () => nextId(store)), prev),
    );
  }
  if (background) {
    const events = coalesceStreamEvents(background.events);
    updateMessages(store, (prev) =>
      events.reduce((acc, ev) => applyEvent(acc, ev, () => nextId(store)), prev),
    );
  }
}

function scheduleStreamFlush() {
  if (streamTimer !== undefined) return;
  streamTimer = window.setTimeout(flushStreams, STREAM_FLUSH_MS);
}

/** 桌面命令流：增量进缓冲，其余事件立即冲刷后应用 */
function enqueueDesktopEvent(store: ChatStore, epoch: number, ev: EngineEvent) {
  if (store.stopRequested || store.epoch !== epoch) return;
  if (ev.type === 'message_delta' || ev.type === 'thought_delta') {
    if (!desktopStream || desktopStream.epoch !== epoch) {
      desktopStream = { epoch, events: [] };
    }
    desktopStream.events.push(ev);
    scheduleStreamFlush();
    return;
  }
  flushStreams();
  updateMessages(store, (prev) => applyEvent(prev, ev, () => nextId(store)));
}

/** 桌面 run_id：优先 crypto.randomUUID，旧 WebView 回退时间戳 + 自增。 */
let runSeq = 0;
function newRunId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return `desktop_${crypto.randomUUID()}`;
    }
  } catch {
    // 非安全上下文等场景拿不到 randomUUID，走回退
  }
  return `desktop_${Date.now()}_${(runSeq += 1)}`;
}

/** 幂等初始化：StrictMode 双挂载 / 切页回来自动复用同一桶，只在首次真正拉历史。
 *  同步置位 initStarted（在第一个 await 之前），第二次调用必然直接返回 —— 同 useSchedulerStore 的写法。
 *  原来那个只服务于卸载的 cancelled 标志刻意删掉：状态已不属于组件，卸载绝不能取消写入。 */
export function ensureInit(store: ChatStore) {
  if (store.initStarted) return;
  store.initStarted = true;
  void runInit(store);
}

/** 加载当前会话历史：未压缩消息（底部）+ 最新压缩块（顶部），按「旧→新」拼接。
 *  修复空会话 bug：压缩后 messages 被清空，历史全在 raw 块，需额外读最新块。 */
async function runInit(store: ChatStore) {
  const epoch = ++store.epoch;
  try {
    const s = await getCurrentSession();
    const base = mapHistoryMessages(s.messages, 'cur'); // 未压缩的最新消息（底部）
    let messages = base;
    let earliestChunk: number | null = null;
    let hasEarlier = false;
    if (s.compress_round > 0) {
      const hist = await loadSessionHistory(s.id, null); // 最新压缩块
      earliestChunk = hist.chunk;
      hasEarlier = hist.has_earlier;
      // 时间线（旧→新）：[块 N 消息] + [current 未压缩消息]
      messages = [...mapHistoryMessages(hist.messages, String(hist.chunk ?? 'cur')), ...base];
    }
    if (epoch === store.epoch) {
      setState(store, { messages, earliestChunk, hasEarlier });
    } else {
      // 期间用户已发消息 / 切了会话：不覆盖 messages，只补游标（否则会冲掉新列表）
      setState(store, { earliestChunk, hasEarlier });
    }
  } catch (e) {
    console.error('get_current_session 失败:', e);
  } finally {
    store.initDone = true;
    setState(store, { initialLoading: false }); // 骨架屏绝不能卡住，失败也要解除
  }
}

/** 拉取会话历史列表（挂载 + 新建/切换/删除/重命名后刷新） */
export async function refreshSessions(store: ChatStore) {
  try {
    setSessions(store, await listSessions());
  } catch (e) {
    console.error('list_sessions 失败:', e);
    setState(store, { sessionsLoading: false });
  }
}

/** 拉取当前会话上下文用量（挂载 + 每轮对话落库后刷新） */
export async function refreshUsage(store: ChatStore) {
  try {
    setState(store, { contextUsage: await getContextUsage() });
  } catch (e) {
    console.error('get_context_usage 失败:', e);
  }
}

/** 新会话：归档当前 + 新建 + 清空列表。
 *  先置 resetting 并乐观清空（立即反馈，避免等后端导致的静止），后端收尾后复位。 */
export async function newSession(store: ChatStore) {
  const st = store.state;
  if (st.isBusy || st.resetting || st.switching) return;
  ++store.epoch; // 整体替换消息列表，作废在飞的 init 写入
  setState(store, { resetting: true, messages: [], earliestChunk: null, hasEarlier: false });
  try {
    await createSession();
    await refreshSessions(store);
    void refreshUsage(store);
  } catch (e) {
    console.error('new_session 失败:', e);
  } finally {
    setState(store, { resetting: false });
  }
}

/** 切换会话：归档当前（后端处理），将目标历史会话设为 active，读取其最新压缩块原始消息渲染 */
export async function switchSession(store: ChatStore, id: string) {
  const st = store.state;
  if (st.isBusy || st.resetting || st.switching || !id || id === st.activeId) return;
  ++store.epoch; // 整体替换消息列表，作废在飞的 init 写入
  setState(store, { switching: true });
  try {
    await switchSessionApi(id);
    const hist = await loadSessionHistory(id, null); // 目标会话最新块
    setState(store, {
      earliestChunk: hist.chunk,
      hasEarlier: hist.has_earlier,
      messages: mapHistoryMessages(hist.messages, String(hist.chunk ?? 'cur')),
    });
    await refreshSessions(store);
    void refreshUsage(store);
  } catch (e) {
    console.error('switch_session 失败:', e);
  } finally {
    setState(store, { switching: false });
  }
}

/** 加载更早一块历史：以最早块号为 before_chunk 请求上一块，拼接到消息区头部 */
export async function loadEarlier(store: ChatStore) {
  const st = store.state;
  if (st.loadingEarlier || !st.hasEarlier || !st.activeId) return;
  setState(store, { loadingEarlier: true });
  try {
    const hist = await loadSessionHistory(st.activeId, st.earliestChunk);
    setState(store, (prev) => ({
      earliestChunk: hist.chunk,
      hasEarlier: hist.has_earlier,
      messages:
        hist.messages.length > 0
          ? [...mapHistoryMessages(hist.messages, String(hist.chunk ?? 'cur')), ...prev.messages]
          : prev.messages,
    }));
  } catch (e) {
    console.error('load_session_history 失败:', e);
  } finally {
    setState(store, { loadingEarlier: false });
  }
}

/** 删除归档会话：后端清目录 + 向量 chunk，随后刷新列表 */
export async function deleteSession(store: ChatStore, id: string) {
  try {
    await deleteSessionApi(id);
  } catch (e) {
    console.error('delete_session 失败:', e);
  } finally {
    await refreshSessions(store);
  }
}

/** 重命名会话标题（活跃或归档均可），随后刷新列表 */
export async function renameSession(store: ChatStore, id: string, title: string) {
  const trimmed = title.trim();
  if (!trimmed) return;
  try {
    await renameSessionApi(id, trimmed);
  } catch (e) {
    console.error('rename_session 失败:', e);
  } finally {
    await refreshSessions(store);
  }
}

/** 停止当前流式生成：置位后端取消标志，chat 提前收尾返回部分结果。
 *  后端会丢弃本轮不落盘，send 的 finally 据此重拉历史，把 UI 同步回存储状态。
 *  刻意不在这里清 isBusy —— 由 send 的 finally 在后端返回后清，避免停止瞬间就允许再次发送。 */
export function stop(store: ChatStore) {
  if (!store.state.isBusy) return;
  store.stopRequested = true;
  if (store.currentRunId) void stopChat(store.currentRunId);
}

/** 主动压缩当前会话上下文：调后端压缩，成功后刷新用量（进度圈归零/下降）。
 *  错误向上抛给调用方（ChatInput 确认框的 handleCompress 兜底 console.error）。 */
export async function compressNow(store: ChatStore) {
  const st = store.state;
  if (st.isBusy || st.compressing || st.resetting || st.switching) return;
  setState(store, { compressing: true });
  try {
    await compressSession();
  } finally {
    setState(store, { compressing: false });
    void refreshUsage(store);
  }
}

/** 发送一条消息：推 user + 空 assistant，跑 chat，逐事件组装进模块状态。
 *  事件回调写的是模块状态而非组件 setState —— 页面卸载后事件照样累积，这是本 store 存在的理由。 */
export async function send(
  store: ChatStore,
  text: string,
  attachments: MessageAttachment[] = [],
) {
  const st = store.state;
  const trimmed = text.trim();
  if ((!trimmed && attachments.length === 0) || st.isBusy || st.compressing || st.resetting || st.switching) {
    return;
  }

  store.stopRequested = false;

  const userMsg: ChatMessage = {
    id: nextId(store),
    role: 'user',
    content: trimmed,
    status: 'done',
    attachments: attachments.length > 0 ? attachments : undefined,
  };
  const assistantId = nextId(store);

  // 守卫判断与置位之间不能有 await，否则连点回车会双发。
  // 守卫读的是模块状态的当前值，不是渲染时的闭包快照 —— 旧实现把 isBusy 放进 useCallback 依赖，
  // 调用方持有一份旧 send 就能并发发两条。
  const runEpoch = ++store.epoch; // 本条会追加消息，作废在飞的 init 写入
  const runId = newRunId();
  store.currentRunId = runId;
  setState(store, {
    messages: [...st.messages, userMsg, newAssistant(assistantId)],
    isBusy: true,
  });

  try {
    await chat(
      trimmed,
      runId,
      attachments.map((a) => ({ path: a.path })),
      (ev) => {
        // 已请求停止或代次已变：本轮消息不会再落盘，忽略陈旧事件，
        // 由 finally 从后端重拉，避免事件与重拉结果互相覆盖。
        enqueueDesktopEvent(store, runEpoch, ev);
      },
    );
  } catch (error) {
    updateMessages(store, (prev) =>
      prev.map((m) =>
        m.id === assistantId && m.role === 'assistant'
          ? { ...m, status: 'error', error: String(error) }
          : m,
      ),
    );
  } finally {
    // 收尾前把缓冲里的增量落进状态；已停止的轮次会在 flushStreams 内被丢弃
    flushStreams();
    if (store.stopRequested) {
      store.stopRequested = false;
      // 后端不保存中断轮次；重拉当前会话，避免 UI 继续显示已丢弃的消息。
      await runInit(store);
    }
    if (store.currentRunId === runId) store.currentRunId = '';
    setState(store, { isBusy: false });
    void refreshUsage(store);
    flushStaleRefresh();
  }
}

/** 读取某个定时任务在主会话里的消息（execution history 视图）。
 *  以 user 消息的 origin.task_id 为组边界：后续 assistant / tool 都归入该组，
 *  直到下一条 user 消息。压缩进历史块的部分不在当前消息里，此视图只展示未压缩段。 */
export async function loadTaskMessages(taskId: string): Promise<ChatMessage[]> {
  const s = await getCurrentSession();
  const picked: HistoryMessage[] = [];
  let inTask = false;
  for (const m of s.messages) {
    if (m.role === 'user') {
      inTask = m.meta?.source === 'task' && m.meta?.task_id === taskId;
      if (inTask) picked.push(m);
    } else if (inTask) {
      picked.push(m);
    }
  }
  return mapHistoryMessages(picked, `task-${taskId}`);
}

/** 后台来源（飞书 / QQ / 定时任务）结束后回填时间线。
 *  session-updated 在每次 run 收尾时广播；桌面自己的 run 由命令流渲染，这里跳过。
 *  仍在执行时不回填，避免和正在累积的流式内容互相覆盖。 */
let syncing = false;
/** 桌面轮次执行期间有后台轮次结束：收尾后补一次回填，避免时间线缺消息 */
let staleWhileBusy = false;

async function refreshFromServer() {
  if (syncing || store.state.isBusy) return;
  syncing = true;
  try {
    await runInit(store);
    await refreshSessions(store);
    void refreshUsage(store);
  } finally {
    syncing = false;
    if (store.state.activeSource) setState(store, { activeSource: null });
  }
}

/** 当前正在实时渲染的后台 run（飞书 / QQ / 任务） */
let bgRunId = '';

/** 后台来源的流式事件：以 run_id 归组追加到同一时间线；结束时从后端回填权威历史 */
function applyBackgroundEvent(payload: SessionEventPayload) {
  const { run_id: runId, event } = payload;
  // 新 run 先开一条空的 assistant 气泡，避免事件串进桌面轮次的气泡
  if (runId !== bgRunId) {
    flushStreams(); // 上一轮（若有）残余增量先落进时间线
    bgRunId = runId;
    const id = nextId(store);
    setState(store, {
      messages: [...store.state.messages, newAssistant(id)],
      activeSource: payload.origin.source,
    });
  }

  // 与桌面流共用合帧策略：增量先进缓冲，done / error / 工具事件立即冲刷
  if (event.type === 'message_delta' || event.type === 'thought_delta') {
    if (!bgStream || bgStream.runId !== runId) {
      bgStream = { runId, events: [] };
    }
    bgStream.events.push(event);
    scheduleStreamFlush();
    return;
  }

  flushStreams();
  updateMessages(store, (prev) => applyEvent(prev, event, () => nextId(store)));

  if (event.type === 'done' || event.type === 'error') {
    bgRunId = '';
    setState(store, { activeSource: null });
    void refreshFromServer();
  }
}

let eventsStarted = false;

/** 应用级订阅：只注册一次，卸载页面不影响。 */
export function initSessionEvents() {
  if (eventsStarted) return;
  eventsStarted = true;
  // 后台来源实时流：桌面轮次进行中时不插入，结束后由 session-updated 统一回填
  listen<SessionEventPayload>('session-event', (event) => {
    const payload = event.payload;
    if (!payload) return;
    if (payload.run_id === store.currentRunId) return; // 自己的命令流已在渲染
    if (store.state.isBusy) return;
    applyBackgroundEvent(payload);
  }).catch((e) => console.error('订阅 session-event 失败:', e));
  listen<{ run_id?: string }>('session-updated', (event) => {
    const runId = event.payload?.run_id ?? '';
    // 自己发起的 run 由命令流渲染，收尾的这条广播跳过
    if (runId && runId === store.currentRunId) return;
    if (store.state.isBusy) {
      staleWhileBusy = true;
      return;
    }
    void refreshFromServer();
  }).catch((e) => console.error('订阅 session-updated 失败:', e));
}

/** send 收尾时调用：执行期间错过的后台轮次补一次回填 */
export function flushStaleRefresh() {
  if (!staleWhileBusy) return;
  staleWhileBusy = false;
  void refreshFromServer();
}

