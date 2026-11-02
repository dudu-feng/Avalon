// 配置协议类型
//
// 与后端 config/types.rs 的 serde 序列化严格对齐（snake_case）。
// 注意：config_path 被后端 #[serde(skip)]，前端不可见、也不需回传。

export type EmbeddingMode = 'local' | 'api';
export type EmbeddingLoadMode = 'lazy' | 'eager';
export type SearchMode = 'semantic' | 'keyword' | 'hybrid';
export type VectorBackend = 'memory' | 'sqlite';
export type FeishuSessionMode = 'isolated' | 'unified';
export type QqBackend = 'official' | 'onebot';
export type QqSessionMode = 'isolated' | 'unified';
export type ObserveMode = 'always' | 'lazy';

/** 本地 OCR 配置，对应后端 [vision.ocr] */
export interface OcrConfig {
  enabled: boolean;
  /** none | onnx */
  engine: string;
  /** 文本置信度达到该值才允许跳过 VLM */
  min_confidence: number;
  /** 模型目录；空字符串 = 数据目录/models/ocr */
  model_dir: string;
  /** 方向分类器：倒置（180°）文本行先转正再识别；需 cls.onnx */
  use_angle_cls: boolean;
  /** 判定倒置的概率阈值 */
  cls_threshold: number;
}

/** 视觉输入配置，对应后端 [vision] */
export interface VisionConfig {
  enabled: boolean;
  /** 视觉模型，指向 models[].name；留空 = 活跃模型（前提 supports_vision） */
  model: string;
  observe_mode: ObserveMode;
  max_edge: number;
  max_bytes: number;
  jpeg_quality: number;
  /** OpenAI 兼容 image_url.detail：auto / low / high；留空则不传 */
  detail: string;
  timeout_secs: number;
  concurrency: number;
  cache_ttl_days: number;
  /** 首次向某个视觉模型发送图片前弹确认；关闭后直接发送 */
  confirm_before_send: boolean;
  ocr: OcrConfig;
}

/** 飞书渠道配置。app_secret 敏感，可被环境变量 AVALON_FEISHU_APP_SECRET 覆盖 */
export interface FeishuConfig {
  enabled: boolean;
  app_id: string;
  app_secret: string;
  domain: string;
  group_require_mention: boolean;
  allow_users: string[];
  session_mode: FeishuSessionMode;
  /**
   * 收到图片消息后、等用户说明意图前发送的询问文本。
   * 留空 = 不询问，收到图片立即交给模型处理
   */
  image_ack_text: string;
  /** 以下为进度表情，值须是飞书 emoji_type 枚举（OnIt / DONE / ERROR…），留空 = 关闭 */
  queued_reaction: string;
  processing_reaction: string;
  done_reaction: string;
  failed_reaction: string;
  rejected_reaction: string;
}

/** QQ 开放平台官方机器人配置，对应 [qq.official] */
export interface QqOfficialConfig {
  app_id: string;
  /** 敏感，可被环境变量 AVALON_QQ_APP_SECRET 覆盖 */
  app_secret: string;
  /** true = sandbox.api.sgroup.qq.com；false = api.bot.qq.com */
  sandbox: boolean;
  /** 群聊 + 单聊消息事件 = 1 << 25 = 33554432 */
  intents: number;
  /** C2C 是否尝试原生流式。群聊始终非流式；关闭仅用于排障 */
  stream_c2c: boolean;
  timeout_secs: number;
}

/** OneBot 11 预留配置。代码未实现，设置页只展示、不启用 */
export interface QqOnebotConfig {
  ws_url: string;
  /** 敏感，可被环境变量 AVALON_QQ_ONEBOT_TOKEN 覆盖 */
  access_token: string;
  self_id: number;
}

/** QQ 渠道配置，对应后端 [qq] */
export interface QqConfig {
  enabled: boolean;
  backend: QqBackend;
  session_mode: QqSessionMode;
  /** 群聊是否只响应 @ 机器人的消息 */
  group_require_mention: boolean;
  /** 允许对话的用户 openid 白名单，空数组 = 不限制 */
  allow_users: string[];
  /** 收到纯图片后、等用户说明意图前发送的询问文本；留空 = 立即处理 */
  image_ack_text: string;
  official: QqOfficialConfig;
  onebot: QqOnebotConfig;
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

/**
 * Agent 基础工具（文件 / 终端）的沙箱配置，对应后端 [tools] 段。
 *
 * workspace_roots 是三态的，不能简化成 string[]：
 *   undefined / null → 后端用默认工作区 data_root/workspace
 *   []               → 禁止全部文件操作
 *   非空数组          → 只允许列出的目录
 * 后端对 None 用了 skip_serializing_if，所以「没配过」时这个字段确实不存在
 */
export interface ToolsConfig {
  workspace_roots?: string[] | null;
  shell_allowlist: string[];
}

/** 模型列表项：连接 + 鉴权 + 模型名，逐模型独立 */
export interface ModelConfig {
  name: string;
  url: string;
  key: string;
  modelname: string;
  /** 是否支持图片输入。必须与提供商的真实能力一致 */
  supports_vision: boolean;
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
  qq: QqConfig;
  search: SearchConfig;
  tools: ToolsConfig;
  vision: VisionConfig;
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
