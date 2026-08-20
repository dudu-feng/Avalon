// 后端 engine 命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// 三条主路径：init_session / save_session / chat（Channel 事件流）。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  ContextUsage,
  CurrentSession,
  EngineEvent,
  HistoryMessage,
  SessionMeta,
} from '../types/chat';

/** 默认会话渠道（应用渠道，对应 current/app.json） */
export const DEFAULT_CHANNEL = 'app';

/** 初始化会话：active 复用 / 否则新建 */
export async function initSession(channelName: string): Promise<void> {
  await invoke('init_session', { channelName });
}

/** 新建会话：归档当前（若非空）+ 创建新的 active 会话，返回新会话完整数据 */
export async function createSession(channelName: string): Promise<CurrentSession> {
  return invoke<CurrentSession>('create_session', { channelName });
}

/** 读取当前会话（含历史消息 + 状态），供 chat 页加载历史 */
export async function getCurrentSession(channelName: string): Promise<CurrentSession> {
  return invoke<CurrentSession>('get_current_session', { channelName });
}

/** 读取当前会话上下文用量（最大输入 token vs 压缩阈值），供圆形进度条展示 */
export async function getContextUsage(channelName: string): Promise<ContextUsage> {
  return invoke<ContextUsage>('get_context_usage', { channelName });
}

/** 归档当前会话：压缩 + 移入 history */
export async function saveSession(channelName: string): Promise<void> {
  await invoke('save_session', { channelName });
}

/** 停止当前会话正在进行的流式生成 */
export async function stopChat(channelName: string): Promise<void> {
  await invoke('stop_chat', { channelName });
}

/** 列出全部会话元信息（active 置顶 + 归档按时间倒序），供会话历史列表 */
export async function listSessions(channelName: string): Promise<SessionMeta[]> {
  return invoke<SessionMeta[]>('list_sessions', { channelName });
}

/** 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据 */
export async function switchSession(channelName: string, id: string): Promise<CurrentSession> {
  return invoke<CurrentSession>('switch_session', { channelName, id });
}

/** 读取某会话最新压缩块的原始消息（供渲染归档历史，后续复用做渐进式加载） */
export async function loadSessionRaw(id: string): Promise<HistoryMessage[]> {
  return invoke<HistoryMessage[]>('load_session_raw', { id });
}

/** 删除归档会话（目录 + 向量库该会话 chunk 一并清理） */
export async function deleteSession(id: string): Promise<void> {
  await invoke('delete_session', { id });
}

/** 重命名会话标题（活跃或归档均可） */
export async function renameSession(channelName: string, id: string, title: string): Promise<void> {
  await invoke('rename_session', { channelName, id, title });
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
