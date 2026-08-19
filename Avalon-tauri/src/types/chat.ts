// 聊天相关类型定义
//
// 分两层：
// 1. 协议类型：与后端 engine/llm/session 的 serde 序列化严格对齐（snake_case）。
// 2. 展示模型：useChat 从事件流组装出的 UI 结构（camelCase，前端内部用）。

// ============ 协议类型（snake_case，与后端 serde 对齐）============

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
}

/** 工具调用（对齐后端 ToolCall） */
export interface ToolCall {
  id: string;
  name: string;
  arguments: unknown;
}

/** chat 命令最终返回（done 事件负载，对齐后端 ChatResult） */
export interface ChatResult {
  thought: string;
  message: string;
  tool_calls: ToolCall[];
  usage: TokenUsage;
}

/** 工具执行记录（对齐后端 ActionRecord，历史消息 action_history 元素） */
export interface ActionRecord {
  action_type: 'tool_call' | 'sub_analysis' | 'finished' | 'error';
  time: string;
  tool_call?: ToolCall | null;
  tool_result?: string | null;
  token_usage: TokenUsage;
}

/** 后端持久化的历史消息（get_current_session 返回的 session 元素） */
export interface HistoryMessage {
  role: 'user' | 'assistant';
  time: string;
  content: string;
  thought?: string | null;
  token_usage?: TokenUsage;
  action_history?: ActionRecord[] | null;
}

/** get_current_session 返回（精简，前端只用这些字段） */
export interface CurrentSession {
  id: string;
  status: 'active' | 'inactive' | 'archived';
  session: HistoryMessage[];
}

/** chat 命令经 Channel 推送的事件（判别联合，按 type 分发） */
export type EngineEvent =
  | { type: 'thought_delta'; delta: string }
  | { type: 'message_delta'; delta: string }
  | { type: 'tool_call'; tool_name: string }
  | { type: 'tool_result'; tool_name: string; success: boolean; result: string }
  | { type: 'done'; result: ChatResult }
  | { type: 'error'; code: number; message: string };

// ============ 展示模型（camelCase，前端内部）============

export type MessageRole = 'user' | 'assistant';

export type MessageStatus = 'streaming' | 'done' | 'error';

/** 单个工具调用摘要（tool_call → tool_result 组装） */
export interface ToolCallRecord {
  toolName: string;
  arguments?: unknown;
  result?: string;
  success?: boolean;
}

/** 会话中的一条消息 */
export interface ChatMessage {
  id: string;
  role: MessageRole;
  status: MessageStatus;
  /** 思考过程（assistant 的 thought，reasoning_content delta 累加；user 为空串） */
  thought: string;
  /** 正文（assistant 的 message，delta 累加） */
  content: string;
  /** 工具调用摘要列表（单模型 ReAct：一次 chat 可多次工具调用，平铺展示） */
  tools: ToolCallRecord[];
  error?: string;
  tokenUsage?: TokenUsage;
}
