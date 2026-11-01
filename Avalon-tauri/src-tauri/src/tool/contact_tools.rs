// 通讯录工具
//
// record_user / update_user / get_user / list_users / get_owner_contact：
// 由智能体主动调用，和维护文件、检索记忆一样是普通工具，不挂在消息准入流程上。
//
// 权限：非主人只能记录、更新、查询自己；list_users 与 get_owner_contact 只给主人。
// 通讯录只存 id / name / profile，不存消息、活跃时间、关系或信任等级。

use serde_json::Value;

use crate::channel::contact::{Contact, ContactStore};
use crate::config::ConfigStore;

use super::ToolContext;

/// 取字符串参数并去空白
fn arg(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        args.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// 取可选字符串参数，保留空串（用于清空 profile）
fn optional_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// 把模型给的 id 规范成带渠道前缀的形式。
/// 已带 feishu:/qq: 前缀的原样返回；裸 id 按当前渠道补前缀；
/// 桌面端没有当前渠道时只接受 ou_ 开头的飞书 id。
fn canonical_id(raw: &str, ctx: &ToolContext) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("参数错误: id 不能为空".to_string());
    }
    if raw.starts_with("feishu:") || raw.starts_with("qq:") {
        return Ok(raw.to_string());
    }
    if let Some(prefix) = ctx.channel_prefix() {
        return Ok(format!("{prefix}:{raw}"));
    }
    if raw.starts_with("ou_") {
        return Ok(format!("feishu:{raw}"));
    }
    Err("参数错误: 桌面端调用请使用带渠道前缀的 id，如 feishu:ou_xxx 或 qq:xxx".to_string())
}

/// 非主人只能操作自己；主人可以操作任意用户
fn ensure_can_touch(ctx: &ToolContext, id: &str) -> Result<(), String> {
    if ctx.is_owner {
        return Ok(());
    }
    match ctx.sender_id.as_deref() {
        Some(self_id) if self_id == id => Ok(()),
        Some(_) => Err("权限不足: 非主人只能操作自己的通讯录记录".to_string()),
        None => Err("权限不足: 无法确认当前用户，不能操作通讯录".to_string()),
    }
}

fn format_contact(c: &Contact) -> String {
    if c.profile.trim().is_empty() {
        format!("{}（{}）\n画像：暂无", c.name, c.id)
    } else {
        format!("{}（{}）\n画像：{}", c.name, c.id, c.profile)
    }
}

/// 记录一个新用户
pub fn record_user(args: &Value, store: &ContactStore, ctx: &ToolContext) -> String {
    let Some(raw_id) = arg(args, &["id", "user_id", "open_id", "sender_id"]) else {
        return "参数错误: 缺少 id".to_string();
    };
    let Some(name) = arg(args, &["name", "nickname", "display_name"]) else {
        return "参数错误: 缺少 name".to_string();
    };
    let profile = optional_arg(args, "profile").unwrap_or("");

    let id = match canonical_id(&raw_id, ctx) {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = ensure_can_touch(ctx, &id) {
        return e;
    }

    match store.record(&id, &name, profile) {
        Ok(c) => {
            log::info!(target: "contact", "已记录用户 {}", c.id);
            format!("已记录用户：{}（{}）", c.name, c.id)
        }
        Err(e) => {
            let message = e.to_string();
            if message.contains("已存在") {
                format!("通讯录: 该用户已存在（{id}），请用 update_user 更新")
            } else {
                format!("通讯录写入失败: {message}")
            }
        }
    }
}

/// 更新已有用户的 name / profile（profile 整体替换）
pub fn update_user(args: &Value, store: &ContactStore, ctx: &ToolContext) -> String {
    let Some(raw_id) = arg(args, &["id", "user_id", "open_id", "sender_id"]) else {
        return "参数错误: 缺少 id".to_string();
    };
    let id = match canonical_id(&raw_id, ctx) {
        Ok(id) => id,
        Err(e) => return e,
    };
    if let Err(e) = ensure_can_touch(ctx, &id) {
        return e;
    }

    let name = optional_arg(args, "name");
    let profile = optional_arg(args, "profile");
    if name.is_none() && profile.is_none() {
        return "参数错误: name 与 profile 至少提供一个".to_string();
    }

    match store.update(&id, name, profile) {
        Ok(c) => {
            log::info!(target: "contact", "已更新用户 {}", c.id);
            format!("已更新用户：{}（{}）", c.name, c.id)
        }
        Err(e) => format!("通讯录更新失败: {e}"),
    }
}

/// 按 id 或 name 查询详情。name 重名时返回候选，不猜
pub fn get_user(args: &Value, store: &ContactStore, ctx: &ToolContext) -> String {
    if let Some(raw_id) = arg(args, &["id", "user_id", "open_id", "sender_id"]) {
        let id = match canonical_id(&raw_id, ctx) {
            Ok(id) => id,
            Err(e) => return e,
        };
        if let Err(e) = ensure_can_touch(ctx, &id) {
            return e;
        }
        return match store.get(&id) {
            Some(c) => format_contact(&c),
            None => format!("通讯录: 没有找到该用户（{id}）"),
        };
    }

    // 按名字枚举只给主人；非主人必须用自己的 id 查，避免用名字探测通讯录
    if !ctx.is_owner {
        return "权限不足: 非主人只能按自己的 id 查询".to_string();
    }
    let Some(name) = arg(args, &["name", "nickname"]) else {
        return "参数错误: 需要 id 或 name".to_string();
    };
    let hits = store.find_by_name(&name);
    match hits.len() {
        0 => format!("通讯录: 没有找到名为「{name}」的用户"),
        1 => format_contact(&hits[0]),
        _ => {
            let lines: Vec<String> = hits
                .iter()
                .map(|c| format!("- {} {}", c.id, c.name))
                .collect();
            format!("找到多个同名用户，请指定 id：\n{}", lines.join("\n"))
        }
    }
}

/// 列出通讯录用户（仅主人）
pub fn list_users(args: &Value, store: &ContactStore, ctx: &ToolContext) -> String {
    if !ctx.is_owner {
        return "权限不足: 通讯录列表只对主人开放".to_string();
    }

    let query = arg(args, &["query", "keyword"]);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 100) as usize;

    let users = store.list(query.as_deref(), limit);
    if users.is_empty() {
        return match query {
            Some(q) => format!("通讯录: 没有匹配「{q}」的用户"),
            None => "通讯录为空".to_string(),
        };
    }

    let mut lines = vec![format!("通讯录（共 {} 人）：", store.len())];
    for c in &users {
        let profile = if c.profile.trim().is_empty() {
            "暂无画像".to_string()
        } else {
            summarize(&c.profile, 80)
        };
        lines.push(format!("- {} {}：{}", c.id, c.name, profile));
    }
    lines.join("\n")
}

/// 获取主人的通讯录详情（仅主人）。两个渠道都配置了主人时都返回
pub fn get_owner_contact(store: &ContactStore, config: &ConfigStore, ctx: &ToolContext) -> String {
    if !ctx.is_owner {
        return "权限不足: 主人通讯录只对主人开放".to_string();
    }

    let cfg = config.get();
    let owners = [
        ("飞书", cfg.feishu.owner_open_id.trim()),
        ("QQ", cfg.qq.owner_open_id.trim()),
    ];
    let mut lines = Vec::new();

    for (label, raw) in owners {
        if raw.is_empty() {
            continue;
        }
        let prefix = if label == "飞书" { "feishu" } else { "qq" };
        let id = if raw.starts_with("feishu:") || raw.starts_with("qq:") {
            raw.to_string()
        } else {
            format!("{prefix}:{raw}")
        };
        match store.get(&id) {
            Some(c) => lines.push(format!(
                "主人（{label}）：{}，{}\n画像：{}",
                c.id,
                c.name,
                if c.profile.trim().is_empty() {
                    "暂无"
                } else {
                    c.profile.as_str()
                }
            )),
            None => lines.push(format!(
                "主人（{label}）：{id}，未记录（可在主人私聊时调用 record_user 补录）"
            )),
        }
    }

    if lines.is_empty() {
        "尚未配置主人。请先私聊一次机器人，或在设置页填写 owner_open_id".to_string()
    } else {
        lines.join("\n\n")
    }
}

fn summarize(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max_chars {
        format!("{}…", flat.chars().take(max_chars).collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn store() -> (ContactStore, PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "avalon-contact-tools-test-{}-{stamp}.json",
            std::process::id()
        ));
        (ContactStore::new(path.clone()), path)
    }

    fn cleanup(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("json.tmp"));
    }

    fn owner_ctx() -> ToolContext {
        ToolContext::owner_desktop()
    }

    fn user_ctx(sender: &str) -> ToolContext {
        ToolContext {
            channel: "feishu".to_string(),
            session_key: "feishu_unified".to_string(),
            sender_id: Some(format!("feishu:{sender}")),
            is_owner: false,
        }
    }

    #[test]
    fn records_user_and_rejects_duplicate() {
        let (s, path) = store();
        let out = record_user(
            &json!({"id": "ou_1", "name": "张三", "profile": "后端"}),
            &s,
            &user_ctx("ou_1"),
        );
        assert!(out.contains("已记录用户"));
        assert!(out.contains("feishu:ou_1"));

        let out = record_user(&json!({"id": "ou_1", "name": "张三"}), &s, &owner_ctx());
        assert!(out.contains("已存在"));
        cleanup(&path);
    }

    #[test]
    fn non_owner_can_only_touch_self() {
        let (s, path) = store();
        let out = record_user(
            &json!({"id": "ou_other", "name": "别人"}),
            &s,
            &user_ctx("ou_self"),
        );
        assert!(out.contains("权限不足"));

        let out = list_users(&json!({}), &s, &user_ctx("ou_self"));
        assert!(out.contains("权限不足"));
        cleanup(&path);
    }

    #[test]
    fn updates_profile_and_reads_back() {
        let (s, path) = store();
        record_user(
            &json!({"id": "ou_1", "name": "张三"}),
            &s,
            &user_ctx("ou_1"),
        );
        let out = update_user(
            &json!({"id": "ou_1", "profile": "关注接口进度"}),
            &s,
            &user_ctx("ou_1"),
        );
        assert!(out.contains("已更新用户"));

        let out = get_user(&json!({"id": "ou_1"}), &s, &user_ctx("ou_1"));
        assert!(out.contains("关注接口进度"));
        cleanup(&path);
    }

    #[test]
    fn owner_lists_contacts_and_resolves_name() {
        let (s, path) = store();
        record_user(
            &json!({"id": "feishu:ou_1", "name": "张三"}),
            &s,
            &owner_ctx(),
        );
        record_user(&json!({"id": "qq:u2", "name": "李四"}), &s, &owner_ctx());

        let out = list_users(&json!({"query": "张"}), &s, &owner_ctx());
        assert!(out.contains("张三"));
        assert!(!out.contains("李四"));

        let out = get_user(&json!({"name": "李四"}), &s, &owner_ctx());
        assert!(out.contains("qq:u2"));
        cleanup(&path);
    }

    #[test]
    fn owner_contact_handles_unrecorded_owner() {
        let (s, path) = store();
        let mut cfg = crate::config::default_config();
        cfg.feishu.owner_open_id = "ou_owner".to_string();
        let cfg = ConfigStore::from_config(cfg);

        let out = get_owner_contact(&s, &cfg, &owner_ctx());
        assert!(out.contains("主人（飞书）"));
        assert!(out.contains("feishu:ou_owner"));
        assert!(out.contains("未记录"));
        cleanup(&path);
    }

    #[test]
    fn name_lookup_with_duplicates_returns_candidates() {
        let (s, path) = store();
        record_user(
            &json!({"id": "feishu:ou_1", "name": "张三"}),
            &s,
            &owner_ctx(),
        );
        record_user(&json!({"id": "qq:u2", "name": "张三"}), &s, &owner_ctx());

        let out = get_user(&json!({"name": "张三"}), &s, &owner_ctx());
        assert!(out.contains("多个同名用户"));
        assert!(out.contains("feishu:ou_1"));
        assert!(out.contains("qq:u2"));
        cleanup(&path);
    }
}
