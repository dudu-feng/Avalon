// 后端 engine 命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// 三条主路径：init_session / save_session / chat（Channel 事件流）。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { Channel, invoke } from '@tauri-apps/api/core';
import type { CurrentSession, EngineEvent } from '../types/chat';

/** 默认会话渠道（应用渠道，对应 current/app.json） */
export const DEFAULT_CHANNEL = 'app';

/** 初始化会话：active 复用 / 否则新建 */
export async function initSession(channelName: string): Promise<void> {
  await invoke('init_session', { channelName });
}

/** 读取当前会话（含历史消息 + 状态），供 chat 页加载历史 */
export async function getCurrentSession(channelName: string): Promise<CurrentSession> {
  return invoke<CurrentSession>('get_current_session', { channelName });
}

/** 归档当前会话：压缩 + 移入 history */
export async function saveSession(channelName: string): Promise<void> {
  await invoke('save_session', { channelName });
}

/** 主对话：跑完整 ReAct，中间态经 onEvent 逐事件回调 */
export async function chat(
  userInput: string,
  channelName: string,
  onEvent: (ev: EngineEvent) => void,
): Promise<void> {
  const channel = new Channel<EngineEvent>();
  channel.onmessage = onEvent;
  await invoke('chat', { userInput, channelName, onEvent: channel });
}
