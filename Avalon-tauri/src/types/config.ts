// 配置协议类型
//
// 与后端 config/types.rs 的 serde 序列化严格对齐（snake_case）。
// 注意：config_path 被后端 #[serde(skip)]，前端不可见、也不需回传。

export type EmbeddingMode = 'local' | 'api';
export type EmbeddingLoadMode = 'lazy' | 'eager';
export type SearchMode = 'semantic' | 'keyword' | 'hybrid';
export type VectorBackend = 'memory' | 'sqlite';

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
}

/** 重建向量库统计（rebuild_memory_index 命令返回） */
export interface RebuildStats {
  cleared: boolean;
  archived_sessions: number;
  active_sessions: number;
  total_chunks: number;
  errors: string[];
}
