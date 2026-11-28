// 流式增量按 60ms 合并，避免每个 token 都触发整棵消息列表重渲染。

import type { EngineEvent, MessageSource } from '../../../../types/chat';
import * as streamPerf from '../../../../lib/streamPerf';
import { store } from './core';
import { applyEvent } from './reducer';
import type { ChatStore } from './types';

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
let bgStream: { runId: string; source: MessageSource; events: EngineEvent[] } | null = null;
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

export function flushStreams() {
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
    const started = streamPerf.streamPerfEnabled ? performance.now() : 0;
    for (const ev of events) {
      applyEvent(store, ev, 'desktop');
    }
    if (streamPerf.streamPerfEnabled) {
      streamPerf.noteFlush(performance.now() - started, events.length);
    }
  }
  if (background) {
    const events = coalesceStreamEvents(background.events);
    const started = streamPerf.streamPerfEnabled ? performance.now() : 0;
    for (const ev of events) {
      applyEvent(store, ev, background.source);
    }
    if (streamPerf.streamPerfEnabled) {
      streamPerf.noteFlush(performance.now() - started, events.length);
    }
  }
}

function scheduleStreamFlush() {
  if (streamTimer !== undefined) return;
  streamTimer = window.setTimeout(flushStreams, STREAM_FLUSH_MS);
}

/** 桌面命令流：增量进缓冲，其余事件立即冲刷后应用 */
export function enqueueDesktopEvent(store: ChatStore, epoch: number, ev: EngineEvent) {
  if (store.stopRequested || store.epoch !== epoch) return;
  if (ev.type === 'message_delta' || ev.type === 'thought_delta') {
    streamPerf.noteDeltaReceived();
    if (!desktopStream || desktopStream.epoch !== epoch) {
      desktopStream = { epoch, events: [] };
    }
    desktopStream.events.push(ev);
    scheduleStreamFlush();
    return;
  }
  flushStreams();
  applyEvent(store, ev, 'desktop');
}

/** 后台来源增量：与桌面流共用合帧定时器，但按 run_id 单独归组 */
export function enqueueBackgroundDelta(runId: string, source: MessageSource, ev: EngineEvent) {
  streamPerf.noteDeltaReceived();
  if (!bgStream || bgStream.runId !== runId) {
    bgStream = { runId, source, events: [] };
  }
  bgStream.events.push(ev);
  scheduleStreamFlush();
}
