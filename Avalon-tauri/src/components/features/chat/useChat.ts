// 聊天状态机 hook —— 唯一调用 chatApi 的地方
//
// 对接后端 engine 的 EngineEvent 流式协议，把扁平事件流组装成
// messages（ChatMessage[]）供展示组件渲染。会话生命周期（init/save）也收在此。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  ChatMessage,
  ContextUsage,
  EngineEvent,
  HistoryMessage,
  SessionMeta,
} from '../../../types/chat';
import {
  chat,
  createSession,
  DEFAULT_CHANNEL,
  deleteSession as deleteSessionApi,
  getContextUsage,
  getCurrentSession,
  listSessions,
  loadSessionRaw,
  renameSession as renameSessionApi,
  stopChat,
  switchSession as switchSessionApi,
} from '../../../lib/chatApi';

export interface UseChatOptions {
  channelName?: string;
}

type AssistantMessage = Extract<ChatMessage, { role: 'assistant' }>;
type ToolMessage = Extract<ChatMessage, { role: 'tool' }>;

/** 后端历史消息（user/assistant/tool 平铺）→ 前端展示消息（逐条直映，无归并） */
function mapHistoryMessages(msgs: HistoryMessage[]): ChatMessage[] {
  const out: ChatMessage[] = [];
  let idx = 0;

  for (const m of msgs) {
    if (m.role === 'user') {
      out.push({ id: `hist-${idx++}`, role: 'user', status: 'done', content: m.content });
    } else if (m.role === 'assistant') {
      out.push({
        id: `hist-${idx++}`,
        role: 'assistant',
        status: 'done',
        thought: m.reasoning_content ?? '',
        content: m.content,
        tokenUsage: m.token_usage,
      });
    } else {
      // role === 'tool'：独立折叠卡片，参数/状态/结果自包含，不依附 assistant
      out.push({
        id: `hist-${idx++}`,
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

/** 事件 → 新消息数组（纯函数，供 setMessages 函数式更新） */
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

export function useChat(options: UseChatOptions = {}) {
  const { channelName = DEFAULT_CHANNEL } = options;
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const [contextUsage, setContextUsage] = useState<ContextUsage | null>(null);
  const [initialLoading, setInitialLoading] = useState(true);
  const [resetting, setResetting] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [sessionsLoading, setSessionsLoading] = useState(true);
  const idRef = useRef(0);

  const nextId = useCallback(() => `msg-${(idRef.current += 1)}`, []);

  // 当前活跃会话 id：由列表推导（后端保证 active 唯一且置顶），切换/新建后随 refreshSessions 更新
  const activeId = useMemo(() => sessions.find((s) => s.status === 'active')?.id ?? '', [sessions]);

  // 拉取会话历史列表（挂载 + 新建/切换/删除/重命名后刷新）
  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await listSessions(channelName));
    } catch (e) {
      console.error('list_sessions 失败:', e);
    } finally {
      setSessionsLoading(false);
    }
  }, [channelName]);

  useEffect(() => {
    refreshSessions();
  }, [refreshSessions]);

  // 拉取当前会话上下文用量（挂载 + 每轮对话落库后刷新）
  const refreshUsage = useCallback(async () => {
    try {
      setContextUsage(await getContextUsage(channelName));
    } catch (e) {
      console.error('get_context_usage 失败:', e);
    }
  }, [channelName]);

  // 挂载时加载当前会话历史：active 复用 → 有历史消息；新建 → 空
  useEffect(() => {
    let cancelled = false;
    getCurrentSession(channelName)
      .then((s) => {
        if (cancelled) return;
        setMessages(mapHistoryMessages(s.messages));
      })
      .catch((e) => console.error('get_current_session 失败:', e))
      .finally(() => {
        if (!cancelled) setInitialLoading(false);
      });
    refreshUsage();
    return () => {
      cancelled = true;
    };
  }, [channelName, refreshUsage]);

  // 新会话：归档当前 + 新建 + 清空前端列表
  // 点击后先置 resetting 并乐观清空（立即反馈，避免等后端导致的静止），后端收尾后复位
  const newSession = useCallback(async () => {
    if (isBusy || resetting || switching) return;
    setResetting(true);
    setMessages([]);
    try {
      await createSession(channelName);
      await refreshSessions();
      refreshUsage();
    } catch (e) {
      console.error('new_session 失败:', e);
    } finally {
      setResetting(false);
    }
  }, [channelName, isBusy, resetting, switching, refreshSessions, refreshUsage]);

  // 切换会话：归档当前（后端处理），将目标历史会话设为 active，读取其最新压缩块原始消息渲染
  const switchSession = useCallback(
    async (id: string) => {
      if (isBusy || resetting || switching || !id || id === activeId) return;
      setSwitching(true);
      try {
        await switchSessionApi(channelName, id);
        const raw = await loadSessionRaw(id);
        setMessages(mapHistoryMessages(raw));
        await refreshSessions();
        refreshUsage();
      } catch (e) {
        console.error('switch_session 失败:', e);
      } finally {
        setSwitching(false);
      }
    },
    [channelName, isBusy, resetting, switching, activeId, refreshSessions, refreshUsage],
  );

  // 删除归档会话：后端清目录 + 向量 chunk，随后刷新列表
  const deleteSession = useCallback(
    async (id: string) => {
      try {
        await deleteSessionApi(id);
      } catch (e) {
        console.error('delete_session 失败:', e);
      } finally {
        await refreshSessions();
      }
    },
    [refreshSessions],
  );

  // 重命名会话标题（活跃或归档均可），随后刷新列表
  const renameSession = useCallback(
    async (id: string, title: string) => {
      const trimmed = title.trim();
      if (!trimmed) return;
      try {
        await renameSessionApi(channelName, id, trimmed);
      } catch (e) {
        console.error('rename_session 失败:', e);
      } finally {
        await refreshSessions();
      }
    },
    [channelName, refreshSessions],
  );

  // 停止当前流式生成：置位后端取消标志，chat 提前收尾返回部分结果
  const stop = useCallback(() => {
    stopChat(channelName);
  }, [channelName]);

  // 发送一条消息：推 user + 空 assistant，跑 chat，逐事件组装
  const send = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!trimmed || isBusy || resetting || switching) return;

      const userMsg: ChatMessage = {
        id: nextId(),
        role: 'user',
        content: trimmed,
        status: 'done',
      };
      const assistantId = nextId();

      setMessages((prev) => [...prev, userMsg, newAssistant(assistantId)]);
      setIsBusy(true);

      try {
        await chat(trimmed, channelName, (ev) => {
          setMessages((prev) => applyEvent(prev, ev, nextId));
        });
      } catch (error) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId && m.role === 'assistant'
              ? { ...m, status: 'error', error: String(error) }
              : m,
          ),
        );
      } finally {
        setIsBusy(false);
        refreshUsage();
      }
    },
    [isBusy, resetting, switching, channelName, nextId, refreshUsage],
  );

  return {
    messages,
    isBusy,
    send,
    newSession,
    stop,
    contextUsage,
    loading: initialLoading || resetting || switching,
    resetting,
    switching,
    sessions,
    sessionsLoading,
    activeId,
    switchSession,
    deleteSession,
    renameSession,
  };
}
