// 渠道通讯录
//
// 只记录用户的三件事：id、name、profile（用户画像）。
// 不记录消息数、最近活跃、群列表、关系、信任等级；用户说过的话在会话历史里，
// 由智能体按 sender_id 自行检索。
//
// 通讯录不参与渠道消息流程：handler 不写这里，只有智能体调用通讯录工具时才写。
// 存储为 data_root/channel/contacts.json，原子写；文件损坏时隔离为 .bad 并降级为空表。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// 用户名最大长度（字符）
pub const NAME_MAX_CHARS: usize = 100;
/// 用户画像最大长度（字符）
pub const PROFILE_MAX_CHARS: usize = 4000;
/// id 最大长度（字符）
pub const ID_MAX_CHARS: usize = 200;

/// 通讯录条目：只有这三个字段
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub profile: String,
}

/// 落盘文件结构。version 预留给后续迁移
#[derive(Debug, Serialize, Deserialize)]
struct ContactFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    users: Vec<Contact>,
}

fn default_version() -> u32 {
    1
}

impl Default for ContactFile {
    fn default() -> Self {
        Self {
            version: 1,
            users: Vec::new(),
        }
    }
}

/// 通讯录存储。跨渠道共享，用 Mutex 保证读改写串行
pub struct ContactStore {
    path: PathBuf,
    inner: Mutex<Vec<Contact>>,
}

impl ContactStore {
    /// 构造：读一次文件进内存。文件不存在/损坏时降级为空表，不阻断应用启动
    pub fn new(path: PathBuf) -> Self {
        let users = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<ContactFile>(&text) {
                Ok(file) => file.users,
                Err(e) => {
                    log::warn!(
                        target: "contact",
                        "通讯录文件解析失败，已隔离并降级为空表: {} - {e}",
                        path.display()
                    );
                    quarantine(&path);
                    Vec::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                log::warn!(
                    target: "contact",
                    "读取通讯录失败，降级为空表: {} - {e}",
                    path.display()
                );
                Vec::new()
            }
        };

        Self {
            path,
            inner: Mutex::new(users),
        }
    }

    /// 记录一个新用户。id 已存在时拒绝，调用方应改用 update
    pub fn record(&self, id: &str, name: &str, profile: &str) -> Result<Contact> {
        let contact = validate_contact(id, name, profile)?;

        let mut users = self.lock();
        if users.iter().any(|u| u.id == contact.id) {
            return Err(anyhow!(
                "该用户已存在（{}），请用 update_user 更新",
                contact.id
            ));
        }
        users.push(contact.clone());
        self.write_locked(&users)?;
        Ok(contact)
    }

    /// 更新已有用户的 name / profile。profile 是整体替换。
    /// 两个字段都为空表示没有实际更新，返回参数错误
    pub fn update(&self, id: &str, name: Option<&str>, profile: Option<&str>) -> Result<Contact> {
        let id = id.trim();
        if id.is_empty() {
            return Err(anyhow!("id 不能为空"));
        }
        if name.is_none() && profile.is_none() {
            return Err(anyhow!("name 与 profile 至少提供一个"));
        }

        let mut users = self.lock();
        let index = users
            .iter()
            .position(|u| u.id == id)
            .ok_or_else(|| anyhow!("没有找到该用户（{id}），请先 record_user"))?;

        if let Some(name) = name {
            let name = name.trim();
            if name.is_empty() {
                return Err(anyhow!("name 不能为空"));
            }
            if name.chars().count() > NAME_MAX_CHARS {
                return Err(anyhow!("name 过长（上限 {NAME_MAX_CHARS} 字符）"));
            }
            users[index].name = name.to_string();
        }
        if let Some(profile) = profile {
            let profile = profile.trim();
            if profile.chars().count() > PROFILE_MAX_CHARS {
                return Err(anyhow!("profile 过长（上限 {PROFILE_MAX_CHARS} 字符）"));
            }
            users[index].profile = profile.to_string();
        }

        let updated = users[index].clone();
        self.write_locked(&users)?;
        Ok(updated)
    }

    /// 删除一个用户。只供前端主人管理，不暴露给智能体工具
    pub fn delete(&self, id: &str) -> Result<()> {
        let id = id.trim();
        if id.is_empty() {
            return Err(anyhow!("id 不能为空"));
        }

        let mut users = self.lock();
        let index = users
            .iter()
            .position(|u| u.id == id)
            .ok_or_else(|| anyhow!("没有找到该用户（{id}）"))?;
        users.remove(index);
        self.write_locked(&users)
    }

    /// 按 id 精确查询
    pub fn get(&self, id: &str) -> Option<Contact> {
        self.lock().iter().find(|u| u.id == id).cloned()
    }

    /// 按 name 精确查询（去空白、大小写不敏感）
    pub fn find_by_name(&self, name: &str) -> Vec<Contact> {
        let needle = name.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.lock()
            .iter()
            .filter(|u| u.name.to_lowercase() == needle)
            .cloned()
            .collect()
    }

    /// 列出用户。query 对 id / name / profile 做模糊匹配；limit 由调用方 clamp
    pub fn list(&self, query: Option<&str>, limit: usize) -> Vec<Contact> {
        let needle = query
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let mut users: Vec<Contact> = self
            .lock()
            .iter()
            .filter(|u| match &needle {
                Some(q) => {
                    u.id.to_lowercase().contains(q)
                        || u.name.to_lowercase().contains(q)
                        || u.profile.to_lowercase().contains(q)
                }
                None => true,
            })
            .cloned()
            .collect();

        users.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        users.truncate(limit);
        users
    }

    /// 通讯录总人数
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Contact>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 锁内序列化 + 原子写（tmp + rename）
    fn write_locked(&self, users: &[Contact]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建通讯录目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(&ContactFile {
            version: 1,
            users: users.to_vec(),
        })
        .context("序列化通讯录失败")?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("写入通讯录临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("原子替换通讯录文件失败: {}", self.path.display()))?;
        Ok(())
    }
}

/// 校验并规范化条目（id/name 去空白，profile 去空白）
fn validate_contact(id: &str, name: &str, profile: &str) -> Result<Contact> {
    let id = id.trim();
    let name = name.trim();
    let profile = profile.trim();

    if id.is_empty() {
        return Err(anyhow!("id 不能为空"));
    }
    if id.chars().count() > ID_MAX_CHARS {
        return Err(anyhow!("id 过长（上限 {ID_MAX_CHARS} 字符）"));
    }
    if name.is_empty() {
        return Err(anyhow!("name 不能为空"));
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(anyhow!("name 过长（上限 {NAME_MAX_CHARS} 字符）"));
    }
    if profile.chars().count() > PROFILE_MAX_CHARS {
        return Err(anyhow!("profile 过长（上限 {PROFILE_MAX_CHARS} 字符）"));
    }

    Ok(Contact {
        id: id.to_string(),
        name: name.to_string(),
        profile: profile.to_string(),
    })
}

/// 损坏文件隔离为 contacts.json.bad-<时间戳>，保留现场供排查
fn quarantine(path: &Path) {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let bad = path.with_extension(format!("json.bad-{stamp}"));
    if let Err(e) = std::fs::rename(path, &bad) {
        log::warn!(
            target: "contact",
            "隔离损坏通讯录失败: {} -> {}: {e}",
            path.display(),
            bad.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (ContactStore, PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "avalon-contact-test-{}-{stamp}.json",
            std::process::id()
        ));
        let store = ContactStore::new(path.clone());
        (store, path)
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("json.tmp"));
    }

    #[test]
    fn records_and_persists_contact() {
        let (store, path) = temp_store();
        let c = store
            .record("feishu:ou_1", "张三", "后端同事")
            .expect("记录应成功");
        assert_eq!(c.name, "张三");
        assert_eq!(store.len(), 1);

        // 重新加载应读回同一条
        let again = ContactStore::new(path.clone());
        assert_eq!(again.get("feishu:ou_1").unwrap().profile, "后端同事");
        cleanup(&path);
    }

    #[test]
    fn rejects_duplicate_id() {
        let (store, path) = temp_store();
        store.record("feishu:ou_1", "张三", "").unwrap();
        let err = store
            .record("feishu:ou_1", "李四", "")
            .unwrap_err()
            .to_string();
        assert!(err.contains("已存在"));
        cleanup(&path);
    }

    #[test]
    fn updates_name_and_profile() {
        let (store, path) = temp_store();
        store.record("feishu:ou_1", "张三", "旧画像").unwrap();
        let c = store
            .update("feishu:ou_1", Some("张三丰"), Some("新画像"))
            .unwrap();
        assert_eq!(c.name, "张三丰");
        assert_eq!(c.profile, "新画像");
        cleanup(&path);
    }

    #[test]
    fn update_requires_at_least_one_field() {
        let (store, path) = temp_store();
        store.record("feishu:ou_1", "张三", "").unwrap();
        let err = store
            .update("feishu:ou_1", None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("至少提供一个"));
        cleanup(&path);
    }

    #[test]
    fn finds_by_name_and_lists_with_query() {
        let (store, path) = temp_store();
        store.record("feishu:ou_1", "张三", "后端").unwrap();
        store.record("qq:u2", "张三", "前端").unwrap();
        store.record("feishu:ou_3", "李四", "产品").unwrap();

        assert_eq!(store.find_by_name("张三").len(), 2);
        assert_eq!(store.list(Some("产品"), 10).len(), 1);
        assert_eq!(store.list(None, 2).len(), 2);
        cleanup(&path);
    }

    #[test]
    fn deletes_contact_and_persists() {
        let (store, path) = temp_store();
        store.record("feishu:ou_1", "张三", "").unwrap();
        store.delete("feishu:ou_1").unwrap();
        assert!(store.get("feishu:ou_1").is_none());

        // 重新加载后仍是删除后的状态
        let again = ContactStore::new(path.clone());
        assert!(again.get("feishu:ou_1").is_none());
        assert!(store.delete("feishu:ou_1").is_err());
        cleanup(&path);
    }

    #[test]
    fn corrupt_file_is_quarantined_and_degrades() {
        let (_, path) = temp_store();
        std::fs::write(&path, "not json").unwrap();
        let store = ContactStore::new(path.clone());
        assert_eq!(store.len(), 0);
        // 原文件被移走，目录里应存在 .bad-* 副本
        let dir = path.parent().unwrap();
        let prefix = path.file_stem().unwrap().to_string_lossy().to_string();
        let has_bad = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{prefix}.json.bad-"))
            });
        assert!(has_bad);
        cleanup(&path);
    }
}
