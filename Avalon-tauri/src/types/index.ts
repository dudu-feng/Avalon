// ============================================================
// 类型定义 —— 前端数据模型
//
// React + TypeScript 中，interface/type 用于定义组件 Props、
// API 返回值、状态数据等。这与 Vue 中定义 interface 完全一致。
// ============================================================

// -- 聊天消息类型 --
// React 中，数据模型通常定义为 interface 或 type。
// interface 可以继承(extends)，type 可以用联合类型(|)，按需选择。
export interface ChatMessage {
  /** 消息唯一 ID */
  id: string;
  /**
   * 消息角色
   * - "user": 用户发送的消息
   * - "assistant": AI 回复的消息
   * - "system": 系统通知消息（如错误提示）
   */
  role: "user" | "assistant" | "system";
  /** 消息文本内容 */
  content: string;
  /** 消息时间戳 (ISO 8601) */
  timestamp: string;
  /** Token 用量信息（仅 assistant 消息有值） */
  tokenUsage?: TokenUsage;
}

// -- LLM 响应中的 Token 统计 --
export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
}

// -- 后端 LLM 调用的统一返回格式 --
// 对应 Rust 的 LlmResponse 结构体
export interface LlmResponse {
  content: string;
  token_usage: TokenUsage;
}

// -- Tauri invoke 调用参数 --
// 对应 Rust 的 ChatParams 结构体
export interface ChatParams {
  system_prompt: string;
  user_input: string;
  chat_history: string; // JSON 字符串
}

// -- 应用配置（对应 Rust AppConfig） --
export interface AppConfig {
  llm: {
    api_key: string;
    model: string;
    base_url: string;
  };
  paths: {
    prompt_file_path: string;
    memory_path: string;
    session_path: string;
    session_index_path: string;
  };
  vector_db: {
    vector_db_path: string;
    model_cache_dir: string;
  };
  embedding: {
    mode: string;
    local_model: string;
    device: string;
    api_key: string;
    api_model: string;
    api_base_url: string;
  };
  session_memory: {
    compress_threshold: number;
    max_chunks: number;
    context_chunks: number;
    search_mode: string;
  };
}

// -- 聊天状态 --
export interface ChatState {
  /** 消息列表 */
  messages: ChatMessage[];
  /** 是否正在等待 AI 回复 */
  loading: boolean;
  /** 错误信息 */
  error: string | null;
}
