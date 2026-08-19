// 聊天状态机 hook —— 唯一调用 chatApi 的地方
//
// 对接后端 engine 的 EngineEvent 流式协议，把扁平事件流组装成
// messages（ChatMessage[]）供展示组件渲染。会话生命周期（init/save）也收在此。

import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ActionRecord,
  ChatMessage,
  EngineEvent,
  HistoryMessage,
  ToolCallRecord,
} from '../../../types/chat';
import { chat, DEFAULT_CHANNEL, getCurrentSession, initSession, saveSession } from '../../../lib/chatApi';

export interface UseChatOptions {
  channelName?: string;
}

/** 历史工具执行记录 → 展示工具摘要（action_history 只保留 tool_call 型） */
function mapActionHistory(history: ActionRecord[] | null | undefined): ToolCallRecord[] {
  if (!history) return [];
  return history
    .filter((r) => r.action_type === 'tool_call' && r.tool_call)
    .map((r) => ({
      toolName: r.tool_call!.name,
      arguments: r.tool_call!.arguments,
      result: r.tool_result ?? undefined,
    }));
}

/** 后端历史消息 → 前端展示消息 */
function mapHistoryMessage(m: HistoryMessage, index: number): ChatMessage {
  const tools = mapActionHistory(m.action_history);
  return {
    id: `hist-${index}`,
    role: m.role,
    status: 'done',
    thought: m.thought ?? '',
    // 执行记录消息：结构化 tools 已承载摘要，正文不再重复渲染
    content: tools.length > 0 ? '' : m.content,
    tools,
    tokenUsage: m.token_usage,
  };
}

/** 定位当前正在组装的 assistant 消息（最后一条 assistant） */
function lastAssistantIndex(messages: ChatMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'assistant') return i;
  }
  return -1;
}

/** 更新末工具摘要（不可变；tool_call/tool_result 由后端严格成对发射） */
function updateLastTool(tools: ToolCallRecord[], patch: Partial<ToolCallRecord>): ToolCallRecord[] {
  const next = [...tools];
  next[next.length - 1] = { ...next[next.length - 1], ...patch };
  return next;
}

/** 事件 → 新消息数组（纯函数，供 setMessages 函数式更新） */
function applyEvent(prev: ChatMessage[], ev: EngineEvent): ChatMessage[] {
  const idx = lastAssistantIndex(prev);
  if (idx < 0) return prev;

  const current = prev[idx];
  let next: ChatMessage = current;

  switch (ev.type) {
    case 'thought_delta':
      next = { ...current, thought: current.thought + ev.delta };
      break;
    case 'message_delta':
      next = { ...current, content: current.content + ev.delta };
      break;
    case 'tool_call':
      next = { ...current, tools: [...current.tools, { toolName: ev.tool_name }] };
      break;
    case 'tool_result':
      next = {
        ...current,
        tools: updateLastTool(current.tools, { result: ev.result, success: ev.success }),
      };
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
  const idRef = useRef(0);

  const nextId = useCallback(() => `msg-${(idRef.current += 1)}`, []);

  // 挂载时加载当前会话历史：active 复用 → 有历史消息；新建 → 空
  useEffect(() => {
    let cancelled = false;
    getCurrentSession(channelName)
      .then((s) => {
        if (cancelled) return;
        setMessages(s.session.map(mapHistoryMessage));
      })
      .catch((e) => console.error('get_current_session 失败:', e));
    return () => {
      cancelled = true;
    };
  }, [channelName]);

  // 新会话：归档当前 + 新建 + 清空前端列表
  const newSession = useCallback(async () => {
    if (isBusy) return;
    try {
      await saveSession(channelName);
      await initSession(channelName);
      setMessages([]);
    } catch (e) {
      console.error('new_session 失败:', e);
    }
  }, [channelName, isBusy]);

  // 发送一条消息：推 user + 空 assistant，跑 chat，逐事件组装
  const send = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!trimmed || isBusy) return;

      const userMsg: ChatMessage = {
        id: nextId(),
        role: 'user',
        content: trimmed,
        thought: '',
        tools: [],
        status: 'done',
      };
      const assistantId = nextId();

      setMessages((prev) => [
        ...prev,
        userMsg,
        {
          id: assistantId,
          role: 'assistant',
          content: '',
          thought: '',
          tools: [],
          status: 'streaming',
        },
      ]);
      setIsBusy(true);

      try {
        await chat(trimmed, channelName, (ev) => {
          setMessages((prev) => applyEvent(prev, ev));
        });
      } catch (error) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId ? { ...m, status: 'error', error: String(error) } : m,
          ),
        );
      } finally {
        setIsBusy(false);
      }
    },
    [isBusy, channelName, nextId],
  );

  return { messages, isBusy, send, newSession };
}
