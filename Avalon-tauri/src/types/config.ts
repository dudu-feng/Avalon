// 配置协议类型
//
// 与后端 config/types.rs 的 serde 序列化严格对齐（snake_case）。
// 注意：config_path 被后端 #[serde(skip)]，前端不可见、也不需回传。

export type EmbeddingMode = 'local' | 'api';
export type EmbeddingLoadMode = 'lazy' | 'eager';
export type SearchMode = 'semantic' | 'keyword' | 'hybrid';
export type VectorBackend = 'memory' | 'sqlite';
export type FeishuSessionMode = 'isolated' | 'unified';

/** 飞书渠道配置。app_secret 敏感，可被环境变量 AVALON_FEISHU_APP_SECRET 覆盖 */
export interface FeishuConfig {
  enabled: boolean;
  app_id: string;
  app_secret: string;
  domain: string;
  group_require_mention: boolean;
  allow_users: string[];
  session_mode: FeishuSessionMode;
  /** 以下为进度表情，值须是飞书 emoji_type 枚举（OnIt / DONE / ERROR…），留空 = 关闭 */
  queued_reaction: string;
  processing_reaction: string;
  done_reaction: string;
  failed_reaction: string;
  rejected_reaction: string;
}

/**
 * 联网搜索（AnySearch）配置，对应后端 [search] 段。
 * 注意与 SearchMode 区分：那个是会话记忆的检索模式，这个是搜互联网。
 * api_key 敏感，可被环境变量 ANYSEARCH_API_KEY 覆盖；留空则匿名调用
 */
export interface SearchConfig {
  enabled: boolean;
  api_key: string;
  base_url: string;
  max_results: number;
  zone: string;
  timeout_secs: number;
  extract_limit: number;
}

/** 模型列表项：连接 + 鉴权 + 模型名，逐模型独立 */
export interface ModelConfig {
  name: string;
  url: string;
  key: string;
  modelname: string;
}

/** 顶层配置：整体映射 Avalon-config.toml */
export interface AppConfig {
  paths: {
    data_root: string;
    file_root: string;
  };
  models: ModelConfig[];
  active_model: string;
  /** 全局 LLM 行为参数（所有模型共享） */
  llm: {
    chat_temperature: number;
    json_temperature: number;
    timeout_secs: number;
  };
  embedding: {
    mode: EmbeddingMode;
    local_model: string;
    device: string;
    load_mode: EmbeddingLoadMode;
    api_key: string;
    api_model: string;
    api_base_url: string;
  };
  session_memory: {
    compress_threshold: number;
    max_chunks: number;
    context_chunks: number;
    search_mode: SearchMode;
  };
  whisper: {
    model_name: string;
    device: string;
  };
  vector: {
    backend: VectorBackend;
  };
  feishu: FeishuConfig;
  search: SearchConfig;
}

/**
 * 渠道运行状态。对应后端 ChannelStatus 的 #[serde(tag = "state")]，
 * 只有 error 变体带 message
 */
export type ChannelStatus =
  | { state: 'disabled' }
  | { state: 'stopped' }
  | { state: 'connecting' }
  | { state: 'running' }
  | { state: 'reconnecting' }
  | { state: 'error'; message: string };

/** 重建向量库统计（rebuild_memory_index 命令返回） */
export interface RebuildStats {
  cleared: boolean;
  archived_sessions: number;
  active_sessions: number;
  total_chunks: number;
  errors: string[];
}

/** 重建进度事件（rebuild_memory_index 经 Channel 推送） */
export interface RebuildProgress {
  processed: number;
  total: number;
  current: string;
}
