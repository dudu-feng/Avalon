// 对话状态 store —— 模块级分桶单例（useSyncExternalStore）。
//
// 为什么状态不放组件里：流式事件经 chatApi 的 Channel 回调送达，回调闭包一旦属于
// 已卸载的组件，React 就会静默丢弃 —— 切页即丢整轮输出。状态提到模块作用域后，
// 页面卸载不影响事件累积，切回来直接渲染。
//
// 为什么按 channel 分桶：useChat 有两个消费者 —— 对话页（app 通道）与定时任务的
// TaskHistoryModal（task.id 通道）。裸单例会让两者互相冲掉消息。

import type {
  ChatMessage,
  ContextUsage,
  EngineEvent,
  HistoryMessage,
  MessageAttachment,
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
      });
    } else if (m.role === 'assistant') {
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'assistant',
        status: 'done',
        thought: m.reasoning_content ?? '',
        content: m.content,
        tokenUsage: m.token_usage,
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
}

export interface ChatStore {
  channel: string;
  state: ChatState;
  listeners: Set<() => void>;
  /** 同步置位（在第一个 await 之前），StrictMode 二次挂载靠它幂等 */
  initStarted: boolean;
  initDone: boolean;
  /** 「历史代次」：整体替换 messages 的动作推进它，用于作废在飞的 init 写入 */
  epoch: number;
  /** msg-* 计数器（桶内自增，不再是组件 useRef —— 卸载重挂不再从 1 重来） */
  idSeq: number;
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
};

const stores = new Map<string, ChatStore>();

function createStore(channel: string): ChatStore {
  const listeners = new Set<() => void>();
  const store: ChatStore = {
    channel,
    state: INITIAL_STATE,
    listeners,
    initStarted: false,
    initDone: false,
    epoch: 0,
    idSeq: 0,
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

/** 桶是否空闲：没有正在进行的生成 / 压缩 / 切换 / 新建 */
function isIdleState(s: ChatState): boolean {
  return !s.isBusy && !s.compressing && !s.resetting && !s.switching;
}

/** 取（或新建）某通道的桶。
 *  「上一轮已跑完、又没有组件在看」的桶是过期快照，丢掉让新桶重新从后端拉，
 *  顺带拿到归档/压缩后的真实历史。跑动中的桶 isBusy=true 绝不会被丢 —— 那正是本 bug 的修复路径。
 *  要求 initDone 是为了不误丢 StrictMode 丢弃渲染时刚建、还没初始化的桶。
 *
 *  释放只在这里做（查询期），刻意不在 setState 里顺手回收：组件从拿到桶到订阅之间隔着
 *  一次提交，若那期间桶被释放，组件就会挂在一个已脱管的桶上，而后来者拿到的是另一个桶 —— 分脑。
 *  查询期回收 + listeners.size === 0 的判定没有这个窗口。 */
export function getChatStore(channel: string): ChatStore {
  const existing = stores.get(channel);
  if (existing && existing.initDone && existing.listeners.size === 0 && isIdleState(existing.state)) {
    stores.delete(channel);
  }
  const hit = stores.get(channel);
  if (hit) return hit;
  const created = createStore(channel);
  stores.set(channel, created);
  return created;
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
    const s = await getCurrentSession(store.channel);
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
    setSessions(store, await listSessions(store.channel));
  } catch (e) {
    console.error('list_sessions 失败:', e);
    setState(store, { sessionsLoading: false });
  }
}

/** 拉取当前会话上下文用量（挂载 + 每轮对话落库后刷新） */
export async function refreshUsage(store: ChatStore) {
  try {
    setState(store, { contextUsage: await getContextUsage(store.channel) });
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
    await createSession(store.channel);
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
    await switchSessionApi(store.channel, id);
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
    await renameSessionApi(store.channel, id, trimmed);
  } catch (e) {
    console.error('rename_session 失败:', e);
  } finally {
    await refreshSessions(store);
  }
}

/** 停止当前流式生成：置位后端取消标志，chat 提前收尾返回部分结果。
 *  刻意不在这里清 isBusy —— 由 send 的 finally 在后端返回后清，避免停止瞬间就允许再次发送。 */
export function stop(store: ChatStore) {
  void stopChat(store.channel);
}

/** 主动压缩当前会话上下文：调后端压缩，成功后刷新用量（进度圈归零/下降）。
 *  错误向上抛给调用方（ChatInput 确认框的 handleCompress 兜底 console.error）。 */
export async function compressNow(store: ChatStore) {
  const st = store.state;
  if (st.isBusy || st.compressing || st.resetting || st.switching) return;
  setState(store, { compressing: true });
  try {
    await compressSession(store.channel);
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
  ++store.epoch; // 本条会追加消息，作废在飞的 init 写入
  setState(store, {
    messages: [...st.messages, userMsg, newAssistant(assistantId)],
    isBusy: true,
  });

  try {
    await chat(
      trimmed,
      store.channel,
      attachments.map((a) => ({ path: a.path })),
      (ev) => {
        updateMessages(store, (prev) => applyEvent(prev, ev, () => nextId(store)));
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
    setState(store, { isBusy: false });
    void refreshUsage(store);
  }
}

