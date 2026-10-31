// 文件存储命令的接口封装
//
// WebView 里的 File 没有可读的本地路径，所以上传走 base64，由后端解码落盘。
// 返回的 StoredFile.path 是相对 file_root 的路径，预览时再经 resolve_file_path
// 换成绝对路径 —— 前端不持有、也不拼接文件根目录。

import { convertFileSrc, invoke } from '@tauri-apps/api/core';

import type { CleanupReport, FileSource, StoredFile } from '../types/file';

export interface SaveAttachmentOptions {
  source?: FileSource;
  sessionId?: string;
  messageId?: string;
}

/** File → 纯 base64（去掉 data URL 前缀） */
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? '');
      const comma = result.indexOf(',');
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error('读取文件失败'));
    reader.readAsDataURL(file);
  });
}

/** 保存聊天附件，返回后端落盘结果（含 sidecar 相对路径） */
export async function saveAttachment(
  file: File,
  options: SaveAttachmentOptions = {},
): Promise<StoredFile> {
  const dataBase64 = await fileToBase64(file);
  return invoke<StoredFile>('save_attachment', {
    fileName: file.name,
    mime: file.type || 'application/octet-stream',
    dataBase64,
    source: options.source ?? 'upload',
    sessionId: options.sessionId ?? null,
    messageId: options.messageId ?? null,
  });
}

/** 相对路径 → 绝对路径（后端做越界校验） */
export function resolveFilePath(relative: string): Promise<string> {
  return invoke<string>('resolve_file_path', { relative });
}

/** 相对路径 → WebView 可访问的 asset URL，用于图片预览 */
export async function toFileUrl(relative: string): Promise<string> {
  return convertFileSrc(await resolveFilePath(relative));
}

/** 清理过期 temp / quarantine；不传 TTL 时后端用默认值 */
export function cleanupFiles(
  options: { tempTtlHours?: number; quarantineTtlDays?: number } = {},
): Promise<CleanupReport> {
  return invoke<CleanupReport>('cleanup_files', {
    tempTtlHours: options.tempTtlHours ?? null,
    quarantineTtlDays: options.quarantineTtlDays ?? null,
  });
}
