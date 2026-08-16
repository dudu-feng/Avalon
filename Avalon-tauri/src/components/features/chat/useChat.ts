// 聊天状态机 hook —— 唯一调用 chatApi 的地方
//
// 管理消息数组、发送流程、错误处理。
// 后端 LLM 尚未完善，当前 USE_MOCK = true 走静态占位；
// 后端完善后将 USE_MOCK 置为 false 即切换到真实调用。

import { useCallback, useRef, useState } from 'react';
import type { ChatMessage } from '../../../types/chat';
import { llmChat } from '../../../lib/chatApi';
import { parseChatResult } from '../../../lib/llmParser';

/** 后端未就绪时的占位开关：true = 静态占位，false = 真实调用 */
const USE_MOCK = true;

const MOCK_REPLY = {
  thought: '当前后端 LLM 尚未接入，这里展示的是静态占位思考过程，用于预览折叠效果。',
  content:
    '这是一条静态占位回复。后端 LLM 完善后，这里会展示模型的真实回答，并解析 thought 与 message 分别呈现思考过程和正文。',
};

/** 把历史序列化为文本（对应后端 chat_history 的当前形态） */
function serializeHistory(messages: ChatMessage[]): string {
  return messages
    .filter((m) => m.status === 'done')
    .map((m) => `${m.role === 'user' ? '用户' : '助手'}：${m.content}`)
    .join('\n');
}

/** 请求一条回复：真实分支调 llm_chat 并解析，占位分支返回静态内容 */
async function requestReply(
  input: string,
  history: ChatMessage[],
  systemPrompt: string,
): Promise<{ content: string; thought: string }> {
  if (!USE_MOCK) {
    const res = await llmChat({
      systemPrompt,
      userInput: input,
      chatHistory: serializeHistory(history),
    });
    const parsed = parseChatResult(res.content);
    if (parsed) return { content: parsed.message, thought: parsed.thought };
    return { content: res.content, thought: '' };
  }

  // 模拟网络延迟，让 pending 状态可见
  await new Promise((resolve) => setTimeout(resolve, 600));
  return { content: MOCK_REPLY.content, thought: MOCK_REPLY.thought };
}

export interface UseChatOptions {
  systemPrompt?: string;
}

export function useChat(options: UseChatOptions = {}) {
  const { systemPrompt = '' } = options;
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  const idRef = useRef(0);

  const nextId = useCallback(() => `msg-${(idRef.current += 1)}`, []);

  const send = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      if (!trimmed || isBusy) return;

      const userMsg: ChatMessage = {
        id: nextId(),
        role: 'user',
        content: trimmed,
        status: 'done',
      };
      const assistantId = nextId();

      setMessages((prev) => [
        ...prev,
        userMsg,
        { id: assistantId, role: 'assistant', content: '', status: 'pending' },
      ]);
      setIsBusy(true);

      try {
        const reply = await requestReply(trimmed, messages, systemPrompt);
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? { ...m, content: reply.content, thought: reply.thought, status: 'done' }
              : m,
          ),
        );
      } catch (error) {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? { ...m, status: 'error', error: String(error) }
              : m,
          ),
        );
      } finally {
        setIsBusy(false);
      }
    },
    [isBusy, messages, systemPrompt, nextId],
  );

  const clear = useCallback(() => setMessages([]), []);

  return { messages, isBusy, send, clear };
}
