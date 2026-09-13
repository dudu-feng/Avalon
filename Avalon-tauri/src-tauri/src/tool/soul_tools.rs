// 灵魂注册表工具实现
//
// list/get/create/update/delete/set_enabled 灵魂条目：让智能体自主维护「自身灵魂」与「用户画像」，
// 实现自进化。走 SoulRegistry（registry.json 索引 + entries/*.md 正文），不碰沙箱文件工具。
// 权限边界（system 完全只读且不展示、seed 可编辑不可删、user 可增删改）由 registry 层保证。

use serde_json::Value;

use crate::soul::{EntryCategory, EntryKind, SoulRegistry};

fn kind_str(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::System => "system",
        EntryKind::Seed => "seed",
        EntryKind::User => "user",
    }
}

fn cat_str(cat: EntryCategory) -> &'static str {
    match cat {
        EntryCategory::Soul => "soul",
        EntryCategory::Profile => "profile",
    }
}

/// 列出可见灵魂条目（不含 system，含 kind/category/title/id），供 agent 查看当前灵魂结构
pub fn list_soul_entries(registry: &SoulRegistry) -> String {
    match registry.list_visible() {
        Ok(entries) => {
            if entries.is_empty() {
                return "灵魂注册表为空".to_string();
            }
            let mut lines = Vec::new();
            for e in &entries {
                lines.push(format!(
                    "- [{}|{}] {} (id: {})",
                    kind_str(e.kind),
                    cat_str(e.category),
                    e.title,
                    e.id
                ));
            }
            format!("当前灵魂条目：\n{}", lines.join("\n"))
        }
        Err(e) => format!("灵魂条目读取失败: {e}"),
    }
}

/// 读取单条灵魂条目 + 正文（查看某条目内容用）
pub fn get_soul_entry(args: &Value, registry: &SoulRegistry) -> String {
    let Some(id) = args.get("id").and_then(Value::as_str) else {
        return "参数错误: 缺少 id".to_string();
    };
    match registry.get(id) {
        Ok(d) => format!(
            "条目「{}」[{}|{}]（id: {}）：\n{}",
            d.entry.title,
            kind_str(d.entry.kind),
            cat_str(d.entry.category),
            d.entry.id,
            d.content
        ),
        Err(e) => format!("灵魂条目读取失败: {e}"),
    }
}

/// 新增灵魂条目（kind=user）
pub fn create_soul_entry(args: &Value, registry: &SoulRegistry) -> String {
    let Some(category) = args
        .get("category")
        .and_then(Value::as_str)
        .and_then(EntryCategory::parse)
    else {
        return "参数错误: category 应为 soul 或 profile".to_string();
    };
    let Some(title) = args.get("title").and_then(Value::as_str) else {
        return "参数错误: 缺少 title".to_string();
    };
    let Some(content) = args.get("content").and_then(Value::as_str) else {
        return "参数错误: 缺少 content".to_string();
    };
    match registry.create(category, title, content) {
        Ok(e) => format!("已新增条目「{}」（id: {}）", e.title, e.id),
        Err(e) => format!("灵魂条目创建失败: {e}"),
    }
}

/// 编辑灵魂条目（title/content 至少一个；system 只读）
pub fn update_soul_entry(args: &Value, registry: &SoulRegistry) -> String {
    let Some(id) = args.get("id").and_then(Value::as_str) else {
        return "参数错误: 缺少 id".to_string();
    };
    let title = args.get("title").and_then(Value::as_str);
    let content = args.get("content").and_then(Value::as_str);
    if title.is_none() && content.is_none() {
        return "参数错误: title 与 content 至少传一个".to_string();
    }
    match registry.update(id, title, content) {
        Ok(e) => format!("已更新条目「{}」", e.title),
        Err(e) => format!("灵魂条目更新失败: {e}"),
    }
}

/// 删除灵魂条目（system 与 seed 不可删）
pub fn delete_soul_entry(args: &Value, registry: &SoulRegistry) -> String {
    let Some(id) = args.get("id").and_then(Value::as_str) else {
        return "参数错误: 缺少 id".to_string();
    };
    match registry.delete(id) {
        Ok(()) => format!("已删除条目（id: {id}）"),
        Err(e) => format!("灵魂条目删除失败: {e}"),
    }
}

/// 切换条目是否注入 system prompt（system 只读）
pub fn set_soul_entry_enabled(args: &Value, registry: &SoulRegistry) -> String {
    let Some(id) = args.get("id").and_then(Value::as_str) else {
        return "参数错误: 缺少 id".to_string();
    };
    let Some(enabled) = args.get("enabled").and_then(Value::as_bool) else {
        return "参数错误: enabled 应为布尔值".to_string();
    };
    match registry.set_enabled(id, enabled) {
        Ok(e) => format!(
            "已{}条目「{}」的注入",
            if enabled { "启用" } else { "停用" },
            e.title
        ),
        Err(e) => format!("灵魂条目更新失败: {e}"),
    }
}
