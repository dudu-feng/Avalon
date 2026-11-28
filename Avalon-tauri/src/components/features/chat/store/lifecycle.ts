// store 生命周期：初始化、会话/用量刷新、后台来源事件回填。

import { listen } from '@tauri-apps/api/event';
import type { SessionEventPayload } from '../../../../types/chat';
import * as streamPerf from '../../../../lib/streamPerf';
import {
  ensureSessionInitialized,
  getContextUsage,
  getCurrentSession,
  listSessions,
  loadSessionHistory,
} from '../../../../lib/chatApi';
import { nextId, setSessions, setState, store } from './core';
import { applyEvent, mapHistoryMessages, newStream, streamToMessage } from './reducer';
import { desktopRunIds } from './runs';
import { enqueueBackgroundDelta, flushStreams } from './streaming';
import type { ChatStore } from './types';

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
export async function runInit(store: ChatStore) {
  const epoch = ++store.epoch;
  try {
    // 首次启动时本页 effect 可能早于 App 的 effect，必须自己等会话就绪
    await ensureSessionInitialized();
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
      // 用后端权威历史整份替换时，任何未提交的流式内容都作废（停止 / 回填场景）
      setState(store, { messages, streaming: null, earliestChunk, hasEarlier });
    } else {
      // 期间用户已发消息 / 切了会话：不覆盖 messages，只补游标（否则会冲掉新列表）
      setState(store, { earliestChunk, hasEarlier });
    }
  } catch (e) {
    console.error('get_current_session 失败:', e);
    // 没有权威历史可替换，残留的流式气泡可能是上一轮异常中断留下的，清掉避免永久停在“生成中”
    if (epoch === store.epoch) setState(store, { streaming: null });
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
let bgPerfToken = 0;

/** 后台来源的流式事件：以 run_id 归组追加到同一时间线；结束时从后端回填权威历史 */
function applyBackgroundEvent(payload: SessionEventPayload) {
  const { run_id: runId, event } = payload;
  // 新 run 先占一个流式槽位，避免事件串进桌面轮次的气泡
  if (runId !== bgRunId) {
    flushStreams(); // 上一轮（若有）残余增量先落进时间线
    ++store.epoch; // 作废在飞的 runInit：它拿到的是上一轮结束时的历史，不能冲掉本轮流式内容
    bgRunId = runId;
    bgPerfToken = streamPerf.beginStreamRun(payload.origin.source);
    const stale = store.state.streaming;
    setState(store, {
      // 单写者前提下一般不会有残留；存在时属于上一轮未收尾的内容，先落成消息
      messages: stale ? [...store.state.messages, streamToMessage(stale, {})] : store.state.messages,
      streaming: newStream(nextId(store), payload.origin.source),
      activeSource: payload.origin.source,
    });
  }

  // 与桌面流共用合帧策略：增量先进缓冲，done / error / 工具事件立即冲刷
  if (event.type === 'message_delta' || event.type === 'thought_delta') {
    enqueueBackgroundDelta(runId, payload.origin.source, event);
    return;
  }

  flushStreams();
  applyEvent(store, event, payload.origin.source);

  if (event.type === 'done' || event.type === 'error') {
    bgRunId = '';
    setState(store, { activeSource: null });
    if (streamPerf.streamPerfEnabled) {
      window.setTimeout(() => streamPerf.endStreamRun(bgPerfToken), 0);
    }
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
    // 自己的命令流已在渲染：桌面 run 收尾后 currentRunId 会被清空，
    // 必须用 desktopRunIds 再兜一层，避免晚到的收尾广播被当成后台轮次重复落库。
    if (payload.run_id === store.currentRunId || desktopRunIds.has(payload.run_id)) return;
    if (store.state.isBusy) return;
    applyBackgroundEvent(payload);
  }).catch((e) => console.error('订阅 session-event 失败:', e));
  listen<{ run_id?: string }>('session-updated', (event) => {
    const runId = event.payload?.run_id ?? '';
    // 自己发起的 run 由命令流渲染，收尾的这条广播跳过
    // （currentRunId 收尾后会被清空，desktopRunIds 才是持久标记）
    if (runId && (runId === store.currentRunId || desktopRunIds.has(runId))) return;
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
