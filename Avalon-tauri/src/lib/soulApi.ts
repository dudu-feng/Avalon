// 灵魂注册表命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// 条目变更写后端后会刷新提示词缓存，下一轮对话即生效。

import { invoke } from '@tauri-apps/api/core';
import type { EntryCategory, SoulEntry, SoulEntryDetail } from '../types/soul';

/** 列出可见灵魂条目（不含 system 基本设定，按 order 排序） */
export async function listSoulEntries(): Promise<SoulEntry[]> {
  return invoke<SoulEntry[]>('list_soul_entries');
}

/** 读取单条灵魂条目 + 正文（详情/编辑模态框用） */
export async function getSoulEntry(id: string): Promise<SoulEntryDetail> {
  return invoke<SoulEntryDetail>('get_soul_entry', { id });
}

/** 新增灵魂条目（kind=user） */
export async function createSoulEntry(
  category: EntryCategory,
  title: string,
  content: string,
): Promise<SoulEntry> {
  return invoke<SoulEntry>('create_soul_entry', { category, title, content });
}

/** 编辑灵魂条目（title/content 至少一个；system 只读） */
export async function updateSoulEntry(
  id: string,
  title?: string,
  content?: string,
): Promise<SoulEntry> {
  const params: Record<string, string> = { id };
  if (title !== undefined) params.title = title;
  if (content !== undefined) params.content = content;
  return invoke<SoulEntry>('update_soul_entry', params);
}

/** 删除灵魂条目（system 与 seed 不可删） */
export async function deleteSoulEntry(id: string): Promise<void> {
  return invoke<void>('delete_soul_entry', { id });
}

/** 切换条目是否注入 system prompt（system 只读） */
export async function setSoulEntryEnabled(id: string, enabled: boolean): Promise<SoulEntry> {
  return invoke<SoulEntry>('set_soul_entry_enabled', { id, enabled });
}
