// 渠道通讯录命令的接口封装（前端主人管理页用）

import { invoke } from '@tauri-apps/api/core';
import type { Contact, ContactList, OwnerContact } from '../types/contact';

/** 列出通讯录。query 对 id / name / 画像做模糊匹配 */
export async function listContacts(query?: string): Promise<ContactList> {
  return invoke<ContactList>('list_contacts', {
    query: query?.trim() || null,
    limit: 500,
  });
}

/** 更新显示名或画像。profile 是整体替换 */
export async function updateContact(
  id: string,
  name: string | undefined,
  profile: string,
): Promise<Contact> {
  return invoke<Contact>('update_contact', {
    id,
    name: name ?? null,
    profile,
  });
}

/** 手工更正某渠道主人。id 必填，昵称可空；通常由首次私聊自动记录 */
export async function updateOwner(
  channel: string,
  id: string,
  name: string,
): Promise<OwnerContact> {
  return invoke<OwnerContact>('update_owner', {
    channel,
    id,
    name,
  });
}

/** 删除通讯录条目（仅前端主人操作） */
export async function deleteContact(id: string): Promise<void> {
  return invoke<void>('delete_contact', { id });
}
