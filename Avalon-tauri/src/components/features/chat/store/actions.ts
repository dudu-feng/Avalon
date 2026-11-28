// 会话动作：新建 / 切换 / 加载更早 / 删除 / 重命名 / 压缩 / 发送 / 停止。

import type {
  ChatMessage,
  HistoryMessage,
  MessageAttachment,
} from '../../../../types/chat';
import * as streamPerf from '../../../../lib/streamPerf';
import {
  chat,
  compressSession,
  createSession,
  deleteSession as deleteSessionApi,
  getCurrentSession,
  loadSessionHistory,
  renameSession as renameSessionApi,
  stopChat,
  switchSession as switchSessionApi,
} from '../../../../lib/chatApi';
import { nextId, setState } from './core';
import {
  flushStaleRefresh,
  refreshSessions,
  refreshUsage,
  runInit,
} from './lifecycle';
import { mapHistoryMessages, newStream, streamToMessage } from './reducer';
import { newRunId, rememberDesktopRun } from './runs';
import { enqueueDesktopEvent, flushStreams } from './streaming';
import type { ChatStore } from './types';

/** 新会话：归档当前 + 新建 + 清空列表。
 *  先置 resetting 并乐观清空（立即反馈，避免等后端导致的静止），后端收尾后复位。 */
export async function newSession(store: ChatStore) {
  const st = store.state;
  // 后台来源正在流式输出时不允许归档：后端会把它归档进当前会话，前端却清空了列表
  if (st.isBusy || st.activeSource !== null || st.resetting || st.switching) return;
  ++store.epoch; // 整体替换消息列表，作废在飞的 init 写入
  setState(store, {
    resetting: true,
    messages: [],
    streaming: null,
    earliestChunk: null,
    hasEarlier: false,
  });
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
  // 后台来源流式期间切会话，旧会话内容会写进新会话的流式槽位
  if (
    st.isBusy ||
    st.activeSource !== null ||
    st.resetting ||
    st.switching ||
    !id ||
    id === st.activeId
  ) {
    return;
  }
  ++store.epoch; // 整体替换消息列表，作废在飞的 init 写入
  setState(store, { switching: true });
  try {
    await switchSessionApi(id);
    const hist = await loadSessionHistory(id, null); // 目标会话最新块
    setState(store, {
      earliestChunk: hist.chunk,
      hasEarlier: hist.has_earlier,
      messages: mapHistoryMessages(hist.messages, String(hist.chunk ?? 'cur')),
      streaming: null,
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
  if (
    st.isBusy ||
    st.activeSource !== null ||
    st.compressing ||
    st.resetting ||
    st.switching
  ) {
    return;
  }
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
  // activeSource ≠ null：后台来源（飞书/QQ/定时任务）正在占用流式槽位，
  // 此时发桌面消息会和后台流写同一个 streaming 对象，双方内容会互相穿插。
  if (
    (!trimmed && attachments.length === 0) ||
    st.isBusy ||
    st.activeSource !== null ||
    st.compressing ||
    st.resetting ||
    st.switching
  ) {
    return;
  }

  store.stopRequested = false;
  const perfToken = streamPerf.beginStreamRun('desktop');

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
  rememberDesktopRun(runId); // 广播按这个 id 识别「自己的轮次」，不依赖会被提前清空的 currentRunId
  store.currentRunId = runId;
  setState(store, {
    messages: [...st.messages, userMsg],
    // 空流式气泡先占位：后续增量只更新它，不再复制 messages 数组
    streaming: newStream(assistantId, 'desktop'),
    isBusy: true,
  });

  // 本轮是否已收尾：invoke 回复与 Channel 事件没有严格先后保证，
  // 收尾后晚到的 done / error 不能再落一次，否则同一条回复会出现两遍。
  let finalized = false;

  try {
    await chat(
      trimmed,
      runId,
      attachments.map((a) => ({ path: a.path })),
      (ev) => {
        // 已请求停止或代次已变：本轮消息不会再落盘，忽略所有晚到事件（含增量），
        // 由 finally 从后端重拉，避免事件与重拉结果互相覆盖。
        if (finalized) return;
        enqueueDesktopEvent(store, runEpoch, ev);
      },
    );
  } catch (error) {
    // 命令本身失败（还没走到事件流）：把当前流式内容落成错误气泡
    const stream = store.state.streaming;
    if (stream) {
      setState(store, {
        messages: [
          ...store.state.messages,
          streamToMessage(stream, { status: 'error', error: String(error) }),
        ],
        streaming: null,
      });
    }
  } finally {
    // 收尾前把缓冲里的增量落进状态；已停止的轮次会在 flushStreams 内被丢弃
    flushStreams();
    // 后端正常收尾一定会发 done；没收到时把已有内容落成普通消息，避免光标一直闪
    if (!store.stopRequested && store.state.streaming) {
      const stream = store.state.streaming;
      setState(store, {
        messages: [...store.state.messages, streamToMessage(stream, {})],
        streaming: null,
      });
    }
    // 兜底提交（或 done 已处理）之后本轮即告收尾：晚到的收尾事件一律丢弃
    finalized = true;
    if (store.stopRequested) {
      store.stopRequested = false;
      // 后端不保存中断轮次；重拉当前会话，避免 UI 继续显示已丢弃的消息。
      await runInit(store);
    }
    if (store.currentRunId === runId) store.currentRunId = '';
    setState(store, { isBusy: false });
    void refreshUsage(store);
    flushStaleRefresh();
    // 等最后一帧绘制完再输出 summary（dev-only）
    if (streamPerf.streamPerfEnabled) {
      window.setTimeout(() => streamPerf.endStreamRun(perfToken), 0);
    }
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
