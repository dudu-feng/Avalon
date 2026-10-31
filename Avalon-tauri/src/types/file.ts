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

/** 一次 temp/quarantine 清理的结果 */
export interface CleanupReport {
  removed_files: number;
  removed_dirs: number;
  freed_bytes: number;
}
