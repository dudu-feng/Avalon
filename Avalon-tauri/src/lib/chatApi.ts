// 后端 engine 命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// 统一会话后所有命令都不再接收 channel：全局只有一个正在使用的 main 会话；
// 渠道只是消息属性，run 由 run_id 标识（停止 / 广播按 run_id 定位）。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  ActiveRuns,
  ChatAttachmentRef,
  ContextUsage,
  CurrentSession,
  EngineEvent,
  LoadHistoryResult,
  SessionMeta,
} from '../types/chat';

/** 初始化会话：active 复用 / 否则新建 main */
export async function initSession(): Promise<void> {
  await invoke('init_session');
}

/** 新建会话：归档当前（若非空）+ 创建新的 active 会话，返回新会话完整数据 */
export async function createSession(): Promise<CurrentSession> {
  return invoke<CurrentSession>('create_session');
}

/** 读取当前会话（含历史消息 + 状态），供 chat 页加载历史 */
export async function getCurrentSession(): Promise<CurrentSession> {
  return invoke<CurrentSession>('get_current_session');
}

/** 读取当前会话上下文用量（最大输入 token vs 压缩阈值），供圆形进度条展示 */
export async function getContextUsage(): Promise<ContextUsage> {
  return invoke<ContextUsage>('get_context_usage');
}

/** 归档当前会话：压缩 + 移入 history */
export async function saveSession(): Promise<void> {
  await invoke('save_session');
}

/** 主动压缩当前会话上下文（不归档），返回是否实际压缩（空会话返回 false） */
export async function compressSession(): Promise<boolean> {
  return invoke<boolean>('compress_session');
}

/** 停止指定 run 的流式生成（后台来源按广播里的 run_id 停止） */
export async function stopChat(runId: string): Promise<void> {
  await invoke('stop_chat', { runId });
}

/** 正在执行 / 排队中的 run 快照（含来源），供输入框提示与按钮禁用 */
export async function activeRuns(): Promise<ActiveRuns> {
  return invoke<ActiveRuns>('active_runs');
}

/** 列出全部会话元信息（active 置顶 + 归档按时间倒序），供会话历史列表 */
export async function listSessions(): Promise<SessionMeta[]> {
  return invoke<SessionMeta[]>('list_sessions');
}

/** 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据 */
export async function switchSession(id: string): Promise<CurrentSession> {
  return invoke<CurrentSession>('switch_session', { id });
}

/** 渐进式加载历史块：beforeChunk 为 null 读最新块，否则读更早一块；返回块号 + 消息 + 是否还有更早 */
export async function loadSessionHistory(
  id: string,
  beforeChunk: number | null,
): Promise<LoadHistoryResult> {
  return invoke<LoadHistoryResult>('load_session_history', { id, beforeChunk });
}

/** 删除归档会话（目录 + 向量库该会话 chunk 一并清理） */
export async function deleteSession(id: string): Promise<void> {
  await invoke('delete_session', { id });
}

/** 重命名会话标题（活跃或归档均可） */
export async function renameSession(id: string, title: string): Promise<void> {
  await invoke('rename_session', { id, title });
}

/** 主对话：跑完整 ReAct，中间态经 onEvent 逐事件回调。
 *  runId 由前端生成并先行登记，停止时按同一 id 定位；
 *  attachments 只传后端相对路径；图片字节由后端读取并预处理。 */
export async function chat(
  userInput: string,
  runId: string,
  attachments: ChatAttachmentRef[],
  onEvent: (ev: EngineEvent) => void,
): Promise<void> {
  const channel = new Channel<EngineEvent>();
  channel.onmessage = onEvent;
  await invoke('chat', { userInput, runId, attachments, onEvent: channel });
}
