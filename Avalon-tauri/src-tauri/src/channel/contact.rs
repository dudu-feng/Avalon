// 渠道通讯录
//
// 存两类条目，刻意分开：
//   - users：普通用户，三件事 id、name、profile（用户画像）。由智能体调用
//     通讯录工具主动维护，渠道 handler 不写。
//   - owners：每个渠道的主人，只有 id 与昵称，没有画像。由渠道 handler 在
//     「私聊 + 通过准入」后自动记录；主人的画像归灵魂模块管理。
//
// 不记录消息数、最近活跃、群列表、关系、信任等级；用户说过的话在会话历史里，
// 由智能体按 sender_id 自行检索。
//
// 存储为 data_root/channel/contacts.json，原子写；文件损坏时隔离为 .bad 并降级为空表。

use std::collections::BTreeMap;
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
/// 支持主人记录的渠道。owner 区按这个顺序稳定输出
pub const OWNER_CHANNELS: [&str; 2] = ["feishu", "qq"];

/// 通讯录普通用户条目：只有这三个字段
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub profile: String,
}

/// 渠道主人条目：只有 id 和昵称，没有画像。
/// 主人的画像存在灵魂模块的 profile 条目里，避免两处各存一份慢慢走样。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerContact {
    /// 带渠道前缀的规范 id，如 feishu:ou_xxx / qq:xxx
    pub id: String,
    /// 昵称。飞书事件通常不提供，允许为空
    #[serde(default)]
    pub name: String,
}

/// 落盘文件结构。version 预留给后续迁移
#[derive(Debug, Serialize, Deserialize)]
struct ContactFile {
    #[serde(default = "default_version")]
    version: u32,
    /// key = 渠道名（feishu / qq）
    #[serde(default)]
    owners: BTreeMap<String, OwnerContact>,
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
            owners: BTreeMap::new(),
            users: Vec::new(),
        }
    }
}

/// 内存中的完整通讯录。owners 与 users 分开放，保证主人不会混进用户列表
#[derive(Default)]
struct ContactData {
    owners: BTreeMap<String, OwnerContact>,
    users: Vec<Contact>,
}

/// 通讯录存储。跨渠道共享，用 Mutex 保证读改写串行
pub struct ContactStore {
    path: PathBuf,
    inner: Mutex<ContactData>,
}

impl ContactStore {
    /// 构造：读一次文件进内存。文件不存在/损坏时降级为空表，不阻断应用启动
    pub fn new(path: PathBuf) -> Self {
        let data = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<ContactFile>(&text) {
                Ok(file) => ContactData {
                    owners: file.owners,
                    users: file.users,
                },
                Err(e) => {
                    log::warn!(
                        target: "contact",
                        "通讯录文件解析失败，已隔离并降级为空表: {} - {e}",
                        path.display()
                    );
                    quarantine(&path);
                    ContactData::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ContactData::default(),
            Err(e) => {
                log::warn!(
                    target: "contact",
                    "读取通讯录失败，降级为空表: {} - {e}",
                    path.display()
                );
                ContactData::default()
            }
        };

        Self {
            path,
            inner: Mutex::new(data),
        }
    }

    // ============ 普通用户 ============

    /// 记录一个新用户。id 已存在时拒绝，调用方应改用 update。
    /// 主人 id 也拒绝：主人只存 id + 昵称，画像归灵魂模块，不能在 users 里重复登记
    pub fn record(&self, id: &str, name: &str, profile: &str) -> Result<Contact> {
        let contact = validate_contact(id, name, profile)?;

        let mut data = self.lock();
        if is_owner_id(&data, &contact.id) {
            return Err(owner_conflict_error());
        }
        if data.users.iter().any(|u| u.id == contact.id) {
            return Err(anyhow!(
                "该用户已存在（{}），请用 update_user 更新",
                contact.id
            ));
        }
        data.users.push(contact.clone());
        self.write_locked(&data)?;
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

        let mut data = self.lock();
        if is_owner_id(&data, id) {
            return Err(owner_conflict_error());
        }
        let index = data
            .users
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
            data.users[index].name = name.to_string();
        }
        if let Some(profile) = profile {
            let profile = profile.trim();
            if profile.chars().count() > PROFILE_MAX_CHARS {
                return Err(anyhow!("profile 过长（上限 {PROFILE_MAX_CHARS} 字符）"));
            }
            data.users[index].profile = profile.to_string();
        }

        let updated = data.users[index].clone();
        self.write_locked(&data)?;
        Ok(updated)
    }

    /// 删除一个用户。只供前端主人管理，不暴露给智能体工具
    pub fn delete(&self, id: &str) -> Result<()> {
        let id = id.trim();
        if id.is_empty() {
            return Err(anyhow!("id 不能为空"));
        }

        let mut data = self.lock();
        let index = data
            .users
            .iter()
            .position(|u| u.id == id)
            .ok_or_else(|| anyhow!("没有找到该用户（{id}）"))?;
        data.users.remove(index);
        self.write_locked(&data)
    }

    /// 按 id 精确查询（只在普通用户区查）
    pub fn get(&self, id: &str) -> Option<Contact> {
        self.lock().users.iter().find(|u| u.id == id).cloned()
    }

    /// 按 name 精确查询（去空白、大小写不敏感，只在普通用户区查）
    pub fn find_by_name(&self, name: &str) -> Vec<Contact> {
        let needle = name.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.lock()
            .users
            .iter()
            .filter(|u| u.name.to_lowercase() == needle)
            .cloned()
            .collect()
    }

    /// 列出普通用户。query 对 id / name / profile 做模糊匹配；limit 由调用方 clamp
    pub fn list(&self, query: Option<&str>, limit: usize) -> Vec<Contact> {
        let needle = query
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let mut users: Vec<Contact> = self
            .lock()
            .users
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

    /// 普通用户总数（不含主人）
    pub fn len(&self) -> usize {
        self.lock().users.len()
    }

    // ============ 主人 ============

    /// 全部已记录的主人，按渠道名索引
    pub fn owners(&self) -> BTreeMap<String, OwnerContact> {
        self.lock().owners.clone()
    }

    /// 某渠道的主人
    pub fn owner(&self, channel: &str) -> Option<OwnerContact> {
        self.lock().owners.get(channel.trim()).cloned()
    }

    /// 某渠道主人的裸 id（去掉渠道前缀），渠道发送接口用这个
    pub fn owner_open_id(&self, channel: &str) -> Option<String> {
        let channel = channel.trim();
        self.owner(channel)
            .map(|o| strip_channel_prefix(&o.id, channel).to_string())
    }

    /// 判断某个发送者是否该渠道主人。raw 可以带前缀，也可以是裸 id
    pub fn is_owner(&self, channel: &str, raw: &str) -> bool {
        let channel = channel.trim();
        let raw = raw.trim();
        if channel.is_empty() || raw.is_empty() {
            return false;
        }
        let id = canonical_id(channel, raw);
        self.lock()
            .owners
            .get(channel)
            .map(|o| o.id == id)
            .unwrap_or(false)
    }

    /// 系统自动记录主人：第一个私聊并通过准入的人生效，已有主人时不覆盖。
    ///
    /// 同 id 再次到达且本地昵称为空时补上昵称（QQ 事件偶尔带 username，
    /// 首次可能拿不到）。返回 true 表示本次首次写入。
    pub fn record_owner(&self, channel: &str, raw_id: &str, name: &str) -> Result<bool> {
        let channel = validate_owner_channel(channel)?;
        let id = validate_owner_id(raw_id, &channel)?;
        let name = normalize_owner_name(name)?;

        let mut data = self.lock();
        if let Some(existing) = data.owners.get(&channel) {
            if existing.id == id && existing.name.is_empty() && !name.is_empty() {
                if let Some(entry) = data.owners.get_mut(&channel) {
                    entry.name = name;
                }
                self.write_locked(&data)?;
            }
            return Ok(false);
        }

        data.owners.insert(channel, OwnerContact { id, name });
        self.write_locked(&data)?;
        Ok(true)
    }

    /// 手工更正主人（前端通讯录页）。id 与昵称整体替换，id 必填
    pub fn set_owner(&self, channel: &str, raw_id: &str, name: &str) -> Result<OwnerContact> {
        let channel = validate_owner_channel(channel)?;
        let id = validate_owner_id(raw_id, &channel)?;
        let name = normalize_owner_name(name)?;

        let owner = OwnerContact { id, name };
        let mut data = self.lock();
        data.owners.insert(channel, owner.clone());
        self.write_locked(&data)?;
        Ok(owner)
    }

    fn lock(&self) -> MutexGuard<'_, ContactData> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 锁内序列化 + 原子写（tmp + rename）
    fn write_locked(&self, data: &ContactData) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建通讯录目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(&ContactFile {
            version: 1,
            owners: data.owners.clone(),
            users: data.users.clone(),
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

/// 主人 id 与普通用户 id 冲突时的统一提示
fn owner_conflict_error() -> anyhow::Error {
    anyhow!("该 id 是渠道主人，主人画像由灵魂模块维护，不在通讯录用户区重复登记")
}

/// 内存中该 id 是否属于某个渠道主人
fn is_owner_id(data: &ContactData, id: &str) -> bool {
    data.owners.values().any(|o| o.id == id)
}

/// 规范化 id：已带 feishu:/qq: 前缀的原样保留，裸 id 按渠道补前缀
fn canonical_id(channel: &str, raw: &str) -> String {
    if raw.starts_with("feishu:") || raw.starts_with("qq:") {
        raw.to_string()
    } else {
        format!("{channel}:{raw}")
    }
}

fn strip_channel_prefix<'a>(id: &'a str, channel: &str) -> &'a str {
    id.strip_prefix(&format!("{channel}:")).unwrap_or(id)
}

fn validate_owner_channel(channel: &str) -> Result<String> {
    let channel = channel.trim();
    if channel.is_empty() {
        return Err(anyhow!("channel 不能为空"));
    }
    if !OWNER_CHANNELS.contains(&channel) {
        return Err(anyhow!("未知渠道（{channel}），只支持 feishu / qq"));
    }
    Ok(channel.to_string())
}

fn validate_owner_id(raw: &str, channel: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(anyhow!("主人 id 不能为空"));
    }
    let id = canonical_id(channel, raw);
    if id.chars().count() > ID_MAX_CHARS {
        return Err(anyhow!("id 过长（上限 {ID_MAX_CHARS} 字符）"));
    }
    Ok(id)
}

/// 主人昵称允许为空；非空时与用户名同一上限
fn normalize_owner_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(anyhow!("name 过长（上限 {NAME_MAX_CHARS} 字符）"));
    }
    Ok(name.to_string())
}

/// 校验并规范化普通用户条目（id/name 去空白，profile 去空白）
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

    #[test]
    fn records_owner_only_once_and_keeps_users_separate() {
        let (store, path) = temp_store();
        assert!(store.record_owner("feishu", "ou_owner", "").unwrap());
        assert_eq!(
            store.owner("feishu").map(|o| o.id).as_deref(),
            Some("feishu:ou_owner")
        );
        assert_eq!(store.len(), 0, "主人不应计入普通用户");

        // 换一个人再来也覆盖不了已有主人
        assert!(!store.record_owner("feishu", "ou_other", "").unwrap());
        assert_eq!(
            store.owner("feishu").map(|o| o.id).as_deref(),
            Some("feishu:ou_owner")
        );

        // 重新加载后主人仍在
        let again = ContactStore::new(path.clone());
        assert_eq!(
            again.owner("feishu").map(|o| o.id).as_deref(),
            Some("feishu:ou_owner")
        );
        cleanup(&path);
    }

    #[test]
    fn owner_name_can_be_filled_later() {
        let (store, path) = temp_store();
        store.record_owner("qq", "U1", "").unwrap();
        assert_eq!(store.owner("qq").unwrap().name, "");

        assert!(!store.record_owner("qq", "U1", "小Q").unwrap());
        assert_eq!(store.owner("qq").unwrap().name, "小Q");
        cleanup(&path);
    }

    #[test]
    fn owner_id_is_reserved_for_owner_section() {
        let (store, path) = temp_store();
        store.record_owner("feishu", "ou_owner", "主人").unwrap();

        let err = store
            .record("feishu:ou_owner", "主人", "画像")
            .unwrap_err()
            .to_string();
        assert!(err.contains("渠道主人"));

        // 用户区已有的普通用户仍可正常更新
        store.record("feishu:ou_guest", "访客", "").unwrap();
        assert!(store.update("feishu:ou_guest", Some("访客2"), None).is_ok());
        cleanup(&path);
    }

    #[test]
    fn owner_matching_accepts_bare_and_prefixed_id() {
        let (store, path) = temp_store();
        store.record_owner("qq", "U1", "").unwrap();
        assert!(store.is_owner("qq", "U1"));
        assert!(store.is_owner("qq", "qq:U1"));
        assert!(store.is_owner("qq", " qq:U1 "));
        assert!(!store.is_owner("qq", "U2"));
        assert!(!store.is_owner("feishu", "U1"));
        assert_eq!(store.owner_open_id("qq").as_deref(), Some("U1"));
        cleanup(&path);
    }

    #[test]
    fn set_owner_replaces_and_persists() {
        let (store, path) = temp_store();
        store.record_owner("feishu", "ou_old", "").unwrap();
        let owner = store.set_owner("feishu", "ou_new", "新主人").unwrap();
        assert_eq!(owner.id, "feishu:ou_new");
        assert_eq!(
            store.owner("feishu").map(|o| o.id).as_deref(),
            Some("feishu:ou_new")
        );

        let again = ContactStore::new(path.clone());
        assert_eq!(again.owner("feishu").unwrap().name, "新主人");
        cleanup(&path);
    }

    #[test]
    fn writing_users_keeps_owners() {
        let (store, path) = temp_store();
        store.record_owner("feishu", "ou_owner", "").unwrap();
        store.record("feishu:ou_1", "张三", "").unwrap();

        let again = ContactStore::new(path.clone());
        assert_eq!(
            again.owner("feishu").map(|o| o.id).as_deref(),
            Some("feishu:ou_owner")
        );
        assert!(again.get("feishu:ou_1").is_some());
        cleanup(&path);
    }
}
