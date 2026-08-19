// 聊天状态机 hook —— 唯一调用 chatApi 的地方
//
// 对接后端 engine 的 EngineEvent 流式协议，把扁平事件流组装成
// messages（ChatMessage[]）供展示组件渲染。会话生命周期（init/save）也收在此。

import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ChatMessage,
  EngineEvent,
  HistoryMessage,
  ToolCallRecord,
} from '../../../types/chat';
import { chat, DEFAULT_CHANNEL, getCurrentSession, initSession, saveSession } from '../../../lib/chatApi';

export interface UseChatOptions {
  channelName?: string;
}

/** 后端历史消息（user/assistant/tool 平铺）→ 前端展示消息（assistant 合并工具摘要） */
function mapHistoryMessages(msgs: HistoryMessage[]): ChatMessage[] {
  const out: ChatMessage[] = [];
  let idx = 0;
  // 带 tool_calls 的 assistant 暂存于此，等 tool 消息补 tools / 最终 assistant 补正文
  let pending: ChatMessage | null = null;

  const flushPending = () => {
    if (pending) {
      out.push(pending);
      pending = null;
    }
  };

  for (const m of msgs) {
    if (m.role === 'user') {
      flushPending();
      out.push({
        id: `hist-${idx++}`,
        role: 'user',
        status: 'done',
        thought: '',
        content: m.content,
        tools: [],
      });
    } else if (m.role === 'assistant') {
      if (m.tool_calls && m.tool_calls.length > 0) {
        // 中间轮（带 tool_calls）：首次创建 pending，后续中间轮忽略（引导语/中间思考丢弃）
        if (!pending) {
          pending = {
            id: `hist-${idx++}`,
            role: 'assistant',
            status: 'done',
            thought: '',
            content: '',
            tools: [],
            tokenUsage: m.token_usage,
          };
        }
      } else if (pending) {
        // 最终正文轮：补全 pending 的正文/思考，push
        pending.content = m.content;
        pending.thought = m.reasoning_content ?? '';
        pending.tokenUsage = m.token_usage;
        flushPending();
      } else {
        // 纯对话（无工具）
        out.push({
          id: `hist-${idx++}`,
          role: 'assistant',
          status: 'done',
          thought: m.reasoning_content ?? '',
          content: m.content,
          tools: [],
          tokenUsage: m.token_usage,
        });
      }
    } else {
      // role === 'tool'
      if (pending) {
        pending.tools.push({ toolName: m.name, result: m.content, success: m.success });
      } else {
        // 孤立 tool（异常）：独立展示为一条工具摘要消息
        out.push({
          id: `hist-${idx++}`,
          role: 'assistant',
          status: 'done',
          thought: '',
          content: '',
          tools: [{ toolName: m.name, result: m.content, success: m.success }],
        });
      }
    }
  }
  flushPending();
  return out;
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
        setMessages(mapHistoryMessages(s.messages));
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
