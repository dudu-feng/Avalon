// 引擎事件 → store 状态的纯归约逻辑。

import type {
  ChatMessage,
  EngineEvent,
  HistoryMessage,
  MessageSource,
  TokenUsage,
} from '../../../../types/chat';
import { nextId, setState } from './core';
import type { ChatStore, StreamState } from './types';

type AssistantMessage = Extract<ChatMessage, { role: 'assistant' }>;
type ToolMessage = Extract<ChatMessage, { role: 'tool' }>;
type CompressMessage = Extract<ChatMessage, { role: 'compress' }>;

/** 后端历史消息（user/assistant/tool 平铺）→ 前端展示消息（逐条直映，无归并）。
 *  key 区分来源（块号 / 'cur'），保证拼接更早块时消息 id 全局唯一。 */
export function mapHistoryMessages(msgs: HistoryMessage[], key: string): ChatMessage[] {
  const out: ChatMessage[] = [];
  let idx = 0;

  for (const m of msgs) {
    if (m.role === 'user') {
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'user',
        status: 'done',
        content: m.content,
        attachments: m.meta?.attachments,
        origin: m.meta ?? null,
        time: m.time,
      });
    } else if (m.role === 'assistant') {
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'assistant',
        status: 'done',
        thought: m.reasoning_content ?? '',
        content: m.content,
        tokenUsage: m.token_usage,
        origin: m.origin ?? null,
        time: m.time,
      });
    } else {
      // role === 'tool'：独立折叠卡片，参数/状态/结果自包含，不依附 assistant
      out.push({
        id: `hist-${key}-${idx++}`,
        role: 'tool',
        tool: {
          id: m.tool_call_id,
          toolName: m.name,
          arguments: m.arguments,
          status: m.success ? 'success' : 'error',
          result: m.content,
        },
        origin: m.origin ?? null,
        time: m.time,
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

/** 新建一个空的流式气泡（首轮由 send 预创建，后续轮由 round_start 触发） */
export function newStream(id: string, source: MessageSource): StreamState {
  return { id, content: '', thought: '', source };
}

/** 流式内容 → 提交后的 assistant 消息（done / error 时沿用同一个 id） */
export function streamToMessage(
  stream: StreamState,
  patch: { status?: 'done' | 'error'; content?: string; error?: string; usage?: TokenUsage },
): AssistantMessage {
  return {
    id: stream.id,
    role: 'assistant',
    status: patch.status ?? 'done',
    thought: stream.thought,
    content: patch.content ?? stream.content,
    error: patch.error,
    tokenUsage: patch.usage,
    // 非桌面来源保留标记：done 提交后来源筛选 / 徽章不必等后端回填才正确
    origin: stream.source === 'desktop' ? null : { source: stream.source },
  };
}

/** 事件路由：增量只改 streaming；轮次边界 / 工具 / 压缩事件才写 messages。 */
export function applyEvent(store: ChatStore, ev: EngineEvent, source: MessageSource) {
  const st = store.state;

  // 增量：只替换流式对象，messages / ui 引用保持不变
  if (ev.type === 'message_delta' || ev.type === 'thought_delta') {
    const stream = st.streaming;
    if (!stream) return;
    const next: StreamState =
      ev.type === 'message_delta'
        ? { ...stream, content: stream.content + ev.delta }
        : { ...stream, thought: stream.thought + ev.delta };
    setState(store, { streaming: next });
    return;
  }

  // 轮次边界：封口上一轮、开新气泡（第一轮复用 send 预创建的空气泡）
  if (ev.type === 'round_start') {
    const stream = st.streaming;
    if (!stream) {
      setState(store, { streaming: newStream(nextId(store), source) });
      return;
    }
    if (!stream.content && !stream.thought) return;
    setState(store, {
      messages: [...st.messages, streamToMessage(stream, {})],
      streaming: newStream(nextId(store), source),
    });
    return;
  }

  // 工具发起：先把当前轮 assistant 落成消息（保证 assistant → tool 的顺序），
  // 再追加独立 tool 卡片；流式槽位留空，下一轮 round_start 再开新气泡。
  if (ev.type === 'tool_call') {
    const stream = st.streaming;
    // 只有本轮真的产出了思考 / 正文才落成 assistant 消息；空气泡直接丢弃，
    // 否则时间线里会多出一条没有任何内容的 assistant 气泡
    const hasContent = !!stream && (!!stream.content || !!stream.thought);
    const messages = stream && hasContent
      ? [...st.messages, streamToMessage(stream, {})]
      : st.messages;
    setState(store, {
      messages: [
        ...messages,
        {
          id: nextId(store),
          role: 'tool',
          tool: { id: ev.id, toolName: ev.tool_name, arguments: ev.arguments, status: 'running' },
        },
      ],
      streaming: null,
    });
    return;
  }

  // 工具结果：更新最后一条 tool 消息的状态与结果
  if (ev.type === 'tool_result') {
    const idx = lastIndexByRole(st.messages, 'tool');
    if (idx < 0) return;
    const copy = [...st.messages];
    const current = copy[idx] as ToolMessage;
    copy[idx] = {
      ...current,
      tool: { ...current.tool, result: ev.result, status: ev.success ? 'success' : 'error' },
    };
    setState(store, { messages: copy });
    return;
  }

  // 压缩开始：追加独立提示气泡（running），compress_done 再回填完成/失败
  if (ev.type === 'compress_start') {
    setState(store, {
      messages: [...st.messages, { id: nextId(store), role: 'compress', status: 'running' }],
    });
    return;
  }

  // 压缩结束：更新最后一条 compress 气泡的状态与错误信息
  if (ev.type === 'compress_done') {
    const idx = lastIndexByRole(st.messages, 'compress');
    if (idx < 0) return;
    const copy = [...st.messages];
    const current = copy[idx] as CompressMessage;
    copy[idx] = {
      ...current,
      status: ev.success ? 'done' : 'error',
      error: ev.error ?? undefined,
    };
    setState(store, { messages: copy });
    return;
  }

  if (ev.type === 'done') {
    const stream = st.streaming;
    if (!stream) {
      // 异常路径（未收到任何流式事件）：用最终结果兜底；空结果不落空气泡
      if (!ev.result.message) {
        setState(store, { streaming: null });
        return;
      }
      const fallback = newStream(nextId(store), source);
      setState(store, {
        messages: [
          ...st.messages,
          streamToMessage(fallback, { content: ev.result.message, usage: ev.result.usage }),
        ],
        streaming: null,
      });
      return;
    }
    // 只提交「当前轮」的流式正文。result.message 是整个 ReAct 循环跨轮累加的正文，
    // 前面的轮次已由 tool_call / round_start 提交过；再用它会重复渲染旧正文
    // （最后一轮无正文时，甚至会落成一条与上一条完全相同的回复）。
    if (!stream.content && !stream.thought) {
      setState(store, { streaming: null });
      return;
    }
    const message = streamToMessage(stream, { usage: ev.result.usage });
    setState(store, { messages: [...st.messages, message], streaming: null });
    return;
  }

  if (ev.type === 'error') {
    const stream = st.streaming ?? newStream(nextId(store), source);
    const message = streamToMessage(stream, { status: 'error', error: ev.message });
    setState(store, { messages: [...st.messages, message], streaming: null });
  }
}
