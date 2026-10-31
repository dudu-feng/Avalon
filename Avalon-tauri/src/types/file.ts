// 文件存储协议类型（对齐后端 file 模块 serde 序列化，snake_case）
//
// 落盘布局见 doc/v0.7/02-file-storage.md。

export type FileKind = 'image' | 'document' | 'audio' | 'video' | 'archive' | 'other';

export type FileSource = 'upload' | 'feishu' | 'generated' | 'web' | 'clipboard' | 'edit';

/** 后端落盘结果。path / meta_path 均相对 file_root，统一用 / 分隔 */
export interface StoredFile {
  id: string;
  path: string;
  kind: FileKind;
  mime: string;
  size: number;
  sha256: string;
  created_at: string;
  original_name?: string;
  meta_path?: string;
}

/** 视觉模型对单张图片的观察结果（analyze_image 命令返回） */
export interface Observation {
  schema: number;
  sha256: string;
  model: string;
  prompt_version: number;
  description: string;
  ocr_text?: string;
  labels: string[];
  elapsed_ms: number;
  created_at: string;
}

/** 视觉能力状态（vision_status 命令返回） */
export interface VisionStatus {
  enabled: boolean;
  /** 实际使用的视觉模型名（models[].name） */
  model?: string | null;
  /** 活跃模型本身是否能看图 */
  active_supports_vision: boolean;
  ocr_enabled: boolean;
}

/** 一次 temp/quarantine 清理的结果 */
export interface CleanupReport {
  removed_files: number;
  removed_dirs: number;
  freed_bytes: number;
}
