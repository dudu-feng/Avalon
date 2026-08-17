// 聊天相关类型定义
//
// 分两层：
// 1. 协议类型：与后端 engine/llm 的 serde 序列化严格对齐（snake_case）。
// 2. 展示模型：useChat 从事件流组装出的 UI 结构（camelCase，前端内部用）。

// ============ 协议类型（snake_case，与后端 serde 对齐）============

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
}

/** 后端持久化的历史消息（get_current_session 返回的 session 元素） */
export interface HistoryMessage {
  role: 'user' | 'assistant';
  content: string;
  thought?: string | null;
  token_usage?: TokenUsage;
}

/** get_current_session 返回（精简，前端只用这些字段） */
export interface CurrentSession {
  id: string;
  status: 'active' | 'inactive' | 'archived';
  session: HistoryMessage[];
}

/** 后端 chat 命令的最终返回（done 事件负载） */
export interface ChatResult {
  thought: string;
  message: string;
  next: 'stop' | 'action';
  action_target?: string | null;
  usage: TokenUsage;
}

/** chat 命令经 Channel 推送的事件（判别联合，按 type 分发） */
export type EngineEvent =
  | { type: 'thought_delta'; delta: string }
  | { type: 'message_delta'; delta: string }
  | { type: 'action_start'; target: string }
  | { type: 'action_step'; analysis: string; next: 'tool_call' | 'sub_analysis' | 'finished' }
  | { type: 'action_tool_call'; tool_name: string; arguments: unknown }
  | { type: 'action_tool_result'; tool_name: string; success: boolean; result: string }
  | { type: 'action_sub_analysis'; analysis: string; sub_analysis: string }
  | { type: 'action_finished'; analysis: string; token_usage: TokenUsage }
  | { type: 'done'; result: ChatResult }
  | { type: 'error'; code: number; message: string };

// ============ 展示模型（camelCase，前端内部）============

export type MessageRole = 'user' | 'assistant';

export type MessageStatus = 'streaming' | 'done' | 'error';

/** 会话中的一条消息 */
export interface ChatMessage {
  id: string;
  role: MessageRole;
  status: MessageStatus;
  /** 思考过程（assistant 的 thought，delta 累加；user 为空串） */
  thought: string;
  /** 正文（assistant 的 message，delta 累加） */
  content: string;
  /** ReAct 动作层（一次 chat 可能多轮进动作层） */
  actions: ActionBlock[];
  error?: string;
  tokenUsage?: TokenUsage;
}

/** 一轮动作层（action_start → action_finished） */
export interface ActionBlock {
  target: string;
  steps: ActionStepRecord[];
}

/** 单个动作步骤（action_step 及其后续事件补全） */
export interface ActionStepRecord {
  analysis: string;
  next: 'tool_call' | 'sub_analysis' | 'finished';
  toolCall?: { toolName: string; arguments: unknown };
  toolResult?: { toolName: string; success: boolean; result: string };
  subAnalysis?: string;
  finished?: { analysis: string; tokenUsage: TokenUsage };
}
