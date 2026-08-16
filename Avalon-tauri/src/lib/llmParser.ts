// LLM 返回 JSON 的健壮解析工具
//
// 模型常返回 markdown 代码块包裹的 JSON 或带前后缀杂质，
// 解析失败时返回 null，由调用方兜底。

import type { ActionResult, ChatResult } from '../types/chat';

/** 剥离 markdown 代码块 / 前后缀杂质，提取 JSON 片段 */
export function extractJson(raw: string): string {
  // 1. 优先匹配 ```json ... ``` 或 ``` ... ```
  const fence = raw.match(/```(?:json)?\s*([\s\S]*?)```/);
  if (fence) return fence[1].trim();

  // 2. 回退：取第一个 { 到最后一个 }
  const start = raw.indexOf('{');
  const end = raw.lastIndexOf('}');
  if (start !== -1 && end > start) return raw.slice(start, end + 1);

  // 3. 都没有则原样返回
  return raw.trim();
}

/** 解析对话层返回的 JSON */
export function parseChatResult(content: string): ChatResult | null {
  try {
    return JSON.parse(extractJson(content)) as ChatResult;
  } catch {
    return null;
  }
}

/** 解析动作层返回的 JSON */
export function parseActionResult(content: string): ActionResult | null {
  try {
    return JSON.parse(extractJson(content)) as ActionResult;
  } catch {
    return null;
  }
}
