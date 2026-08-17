// 聊天状态机 hook —— 唯一调用 chatApi 的地方
//
// 对接后端 engine 的 EngineEvent 流式协议，把扁平事件流组装成
// messages（ChatMessage[]）供展示组件渲染。会话生命周期（init/save）也收在此。

import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ActionBlock,
  ActionStepRecord,
  ChatMessage,
  EngineEvent,
  HistoryMessage,
} from '../../../types/chat';
import { chat, DEFAULT_CHANNEL, getCurrentSession, initSession, saveSession } from '../../../lib/chatApi';

export interface UseChatOptions {
  channelName?: string;
}

/** 后端历史消息 → 前端展示消息（action_history 本期暂不还原） */
function mapHistoryMessage(m: HistoryMessage, index: number): ChatMessage {
  return {
    id: `hist-${index}`,
    role: m.role,
    status: 'done',
    thought: m.thought ?? '',
    content: m.content,
    actions: [],
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

/** 向末 ActionBlock 追加一个步骤（不可变） */
function pushStep(actions: ActionBlock[], step: ActionStepRecord): ActionBlock[] {
  const next = [...actions];
  const last = next[next.length - 1];
  next[next.length - 1] = { ...last, steps: [...last.steps, step] };
  return next;
}

/** 更新末 ActionBlock 的末步骤（不可变） */
function updateLastStep(actions: ActionBlock[], patch: Partial<ActionStepRecord>): ActionBlock[] {
  const next = [...actions];
  const last = next[next.length - 1];
  const steps = [...last.steps];
  steps[steps.length - 1] = { ...steps[steps.length - 1], ...patch };
  next[next.length - 1] = { ...last, steps };
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
    case 'action_start':
      next = { ...current, actions: [...current.actions, { target: ev.target, steps: [] }] };
      break;
    case 'action_step':
      next = { ...current, actions: pushStep(current.actions, { analysis: ev.analysis, next: ev.next }) };
      break;
    case 'action_tool_call':
      next = {
        ...current,
        actions: updateLastStep(current.actions, {
          toolCall: { toolName: ev.tool_name, arguments: ev.arguments },
        }),
      };
      break;
    case 'action_tool_result':
      next = {
        ...current,
        actions: updateLastStep(current.actions, {
          toolResult: { toolName: ev.tool_name, success: ev.success, result: ev.result },
        }),
      };
      break;
    case 'action_sub_analysis':
      next = { ...current, actions: updateLastStep(current.actions, { subAnalysis: ev.sub_analysis }) };
      break;
    case 'action_finished':
      next = {
        ...current,
        actions: updateLastStep(current.actions, {
          finished: { analysis: ev.analysis, tokenUsage: ev.token_usage },
        }),
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
        actions: [],
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
          actions: [],
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
