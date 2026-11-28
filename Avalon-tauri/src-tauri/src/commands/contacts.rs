// IPC：渠道通讯录与主人管理。

use std::collections::BTreeMap;
use std::sync::Arc;

use tauri::State;

use crate::channel::contact::{Contact, ContactStore, OwnerContact};

/// 通讯录列表：total 是全量人数，users 是本次匹配结果，owners 按渠道索引
#[derive(serde::Serialize)]
pub struct ContactList {
    pub total: usize,
    pub users: Vec<Contact>,
    pub owners: BTreeMap<String, OwnerContact>,
}

/// 列出通讯录（前端主人管理页用）。query 对 id / name / profile 模糊匹配。
/// owners 不受 query 影响，始终返回全部已记录的主人
#[tauri::command]
pub fn list_contacts(
    query: Option<String>,
    limit: Option<usize>,
    contacts: State<'_, Arc<ContactStore>>,
) -> Result<ContactList, String> {
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    Ok(ContactList {
        total: contacts.len(),
        users: contacts.list(query.as_deref(), limit),
        owners: contacts.owners(),
    })
}

/// 手工更正某渠道主人（前端通讯录页）。id 必填，昵称可空；
/// 通常由渠道首次私聊自动记录，这里只用于纠正
#[tauri::command]
pub fn update_owner(
    channel: String,
    id: String,
    name: Option<String>,
    contacts: State<'_, Arc<ContactStore>>,
) -> Result<OwnerContact, String> {
    contacts
        .set_owner(&channel, &id, name.as_deref().unwrap_or(""))
        .map_err(|e| format!("{e:#}"))
}

/// 更新通讯录条目。profile 是整体替换；name / profile 至少提供一个
#[tauri::command]
pub fn update_contact(
    id: String,
    name: Option<String>,
    profile: Option<String>,
    contacts: State<'_, Arc<ContactStore>>,
) -> Result<Contact, String> {
    contacts
        .update(&id, name.as_deref(), profile.as_deref())
        .map_err(|e| format!("{e:#}"))
}

/// 删除通讯录条目。只供前端主人管理，智能体工具没有删除权限
#[tauri::command]
pub fn delete_contact(id: String, contacts: State<'_, Arc<ContactStore>>) -> Result<(), String> {
    contacts.delete(&id).map_err(|e| format!("{e:#}"))
}
