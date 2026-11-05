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
  /** 思考 token（completion_tokens_details.reasoning_tokens，DeepSeek reasoner） */
  reasoning_tokens?: number;
  /** 缓存命中的输入 token（prompt_tokens_details.cached_tokens） */
  cached_tokens?: number;
}

/** 发给后端 chat 命令的图片附件引用：只信任路径，其余信息由后端从 sidecar 补齐 */
export interface ChatAttachmentRef {
  path: string;
}

/** 落进 user 消息 meta.attachments 的附件快照（对齐后端 vision::Attachment） */
export interface MessageAttachment {
  path: string;
  mime: string;
  size: number;
  sha256: string;
  /** 原图尺寸；前端乐观渲染时可能尚未知，后端快照会补齐 */
  width?: number;
  height?: number;
  original_name?: string;
  /** 已生成的图像观察；后续轮次靠它回忆图片内容 */
  observation?: string;
  ocr_text?: string;
  /** VLM 短标签；旧会话可能没有 */
  labels?: string[];
  /** 生成观察的模型名 */
  observation_model?: string;
  /** 观察生成时间（RFC3339） */
  observed_at?: string;
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
  /** 生成本结果的模型名（供按模型归集统计） */
  model: string;
}

/** 后端持久化的历史消息（get_current_session 返回的 messages 元素，判别联合） */
export type HistoryMessage =
  | {
      role: 'user';
      time: string;
      content: string;
      /** 渠道来源与附件快照（后端 Message::User.meta） */
      meta?: { attachments?: MessageAttachment[]; [key: string]: unknown } | null;
    }
  | {
      role: 'assistant';
      time: string;
      content: string;
      reasoning_content?: string | null;
      tool_calls?: ToolCall[] | null;
      token_usage: TokenUsage;
      model?: string;
    }
  | {
      role: 'tool';
      time: string;
      tool_call_id: string;
      name: string;
      /** 工具调用参数（自包含，历史/旧数据可能缺失） */
      arguments?: unknown;
      success: boolean;
      content: string;
      /** 大结果落盘后的引用（后端 Message::Tool.artifact） */
      artifact?: { id: string; chars: number; bytes: number } | null;
    };

/** get_current_session 返回（精简，前端只用这些字段） */
export interface CurrentSession {
  id: string;
  status: 'active' | 'inactive' | 'archived';
  messages: HistoryMessage[];
  /** 已压缩轮数 = 已写入的最大普通块号（0 表示从未压缩，无历史块） */
  compress_round: number;
}

/** load_session_history 命令返回（渐进式加载历史块） */
export interface LoadHistoryResult {
  /** 本次返回的块号；null 表示无更早历史可加载 */
  chunk: number | null;
  messages: HistoryMessage[];
  /** 是否还有比本块更早的块（前端据此决定是否继续显示加载入口） */
  has_earlier: boolean;
}

/** 会话列表元信息（list_sessions 返回，对齐后端 SessionMeta） */
export interface SessionMeta {
  id: string;
  title: string;
  status: 'active' | 'inactive' | 'archived';
  /** 创建时间（epoch 秒，由 id 时间戳解析，供前端时间分组） */
  created_at: number;
}

/** 当前会话上下文用量（get_context_usage 命令返回） */
export interface ContextUsage {
  used_tokens: number;
  threshold: number;
}

/** chat 命令经 Channel 推送的事件（判别联合，按 type 分发） */
export type EngineEvent =
  | { type: 'round_start' }
  | { type: 'thought_delta'; delta: string }
  | { type: 'message_delta'; delta: string }
  | { type: 'tool_call'; id: string; tool_name: string; arguments?: unknown }
  | { type: 'tool_result'; tool_name: string; success: boolean; result: string }
  | { type: 'done'; result: ChatResult }
  | { type: 'compress_start' }
  | { type: 'compress_done'; success: boolean; error?: string | null }
  | { type: 'error'; code: number; message: string };

// ============ 展示模型（camelCase，前端内部）============

export type MessageStatus = 'streaming' | 'done' | 'error';

/** 单个工具调用记录（tool 消息的自包含载体：参数 + 状态 + 结果） */
export interface ToolCallRecord {
  /** 工具调用唯一 id（对齐后端 tool_call_id） */
  id?: string;
  toolName: string;
  arguments?: unknown;
  /** 执行状态：running（执行中）/ success / error */
  status: 'running' | 'success' | 'error';
  result?: string;
}

/**
 * 会话中的一条消息（三态判别联合，对齐后端 messages 的 user/assistant/tool 平铺）
 * - user：正文
 * - assistant：思考 + 正文 + 用量（一轮大模型调用一个气泡）
 * - tool：独立折叠卡片，参数/状态/结果自包含，不依附 assistant
 */
export type ChatMessage =
  | {
      id: string;
      role: 'user';
      status: MessageStatus;
      content: string;
      /** 本轮图片附件（历史消息从 meta.attachments 恢复） */
      attachments?: MessageAttachment[];
    }
  | {
      id: string;
      role: 'assistant';
      status: MessageStatus;
      thought: string;
      content: string;
      error?: string;
      tokenUsage?: TokenUsage;
    }
  | { id: string; role: 'tool'; tool: ToolCallRecord }
  | {
      id: string;
      /** 压缩上下文提示（运行时过程提示，不入历史）：running=压缩中 / done=完成 / error=失败 */
      role: 'compress';
      status: 'running' | 'done' | 'error';
      error?: string;
    };
