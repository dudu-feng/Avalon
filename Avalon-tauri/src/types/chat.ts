// 聊天相关类型定义
//
// 与后端 src-tauri/src/llm/types.rs 的返回结构对应，
// 并补充前端会话状态所需的字段。

export type MessageRole = 'user' | 'assistant';

export type MessageStatus = 'pending' | 'done' | 'error';

/** 会话中的一条消息（前端展示用） */
export interface ChatMessage {
  id: string;
  role: MessageRole;
  /** 展示正文（assistant 对应解析后的 message 字段） */
  content: string;
  /** 思考过程（assistant 的 thought 字段，可选折叠展示） */
  thought?: string;
  status: MessageStatus;
  error?: string;
  tokenUsage?: TokenUsage;
}

/** 对应 Rust LlmResponse.token_usage */
export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
}

/** 后端统一返回结构（对应 Rust LlmResponse） */
export interface LlmResponse {
  /** LLM 返回的原始文本（对话层为 JSON 字符串） */
  content: string;
  token_usage: TokenUsage;
}

/** 对话层返回 JSON 的解析结果 */
export interface ChatResult {
  thought: string;
  message: string;
  next: 'action' | 'stop';
  action_target?: string;
}

/** 动作层返回 JSON 的解析结果 */
export interface ActionResult {
  analysis: string;
  next: 'tool_call' | 'sub_analysis' | 'finished';
  tool_call?: { name: string; arguments: Record<string, unknown> };
  sub_analysis?: string;
}
