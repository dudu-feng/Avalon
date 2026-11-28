// 旧数据迁移：工具链清洗、分渠道 current → 统一 main 会话。
//
// 迁移只写标记成功的批次；任何文件失败都不写标记，下次启动重试，失败文件不删除。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::files::temp_path;
use super::{
    FileSessionStore, ACTIVE_SESSION_KEY, TOOLCHAIN_CLEANUP_MARKER, UNIFIED_SESSION_MARKER,
};
use crate::session::compress::build_transcript;
use crate::session::context::{
    build_history_messages, estimate_messages_tokens, estimate_text_tokens, ContextBudget,
};
use crate::session::migrate::{
    dedup_key, fill_group_origin, group_turns, normalize_legacy_trust, sanitize_tool_chains,
    MigrationReport,
};
use crate::session::now_id_ts;
use crate::session::types::{Message, SessionData, SessionStats, SessionStatus};

impl FileSessionStore {
    /// 一次性清洗旧版本取消留下的不完整工具链。
    ///
    /// 旧引擎停止时仍会写入已生成的部分轨迹，可能留下 assistant(tool_calls)
    /// 缺少对应 tool 结果的半条链。新投影不做运行时配对修复（见
    /// `context::build_history_messages`），因此启动时把 current / history index /
    /// raw 块统一清洗一次。标记文件保证只扫描一次；清洗幂等，失败时下次启动重试。
    pub fn cleanup_legacy_tool_chains(&self) -> Result<usize> {
        let marker = self.session_path().join(TOOLCHAIN_CLEANUP_MARKER);
        if marker.is_file() {
            return Ok(0);
        }

        let mut removed = 0usize;
        let mut touched = 0usize;

        // ① 活跃会话：current/{channel}.json
        let current_dir = self.session_path().join("current");
        if current_dir.is_dir() {
            for entry in std::fs::read_dir(&current_dir)
                .with_context(|| format!("读取 current 目录失败: {}", current_dir.display()))?
            {
                let path = entry.context("读取 current 条目失败")?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                match self.read_session_file(&path) {
                    Ok(mut data) => {
                        let n = sanitize_tool_chains(&mut data.messages);
                        if n > 0 {
                            // 迁移前 current 仍按渠道分文件，必须写回原路径而非 main
                            self.write_session_file(&path, &data)?;
                            removed += n;
                            touched += 1;
                        }
                    }
                    Err(e) => log::warn!(
                        target: "session",
                        "跳过无法解析的会话文件 {}: {e:#}",
                        path.display()
                    ),
                }
            }
        }

        // ② 归档会话：history/{id}/index.json + raw/{chunk}.json
        let history = self.history_dir();
        if history.is_dir() {
            for entry in std::fs::read_dir(&history)
                .with_context(|| format!("读取 history 目录失败: {}", history.display()))?
            {
                let dir = entry.context("读取 history 条目失败")?.path();
                if !dir.is_dir() {
                    continue;
                }

                let index = dir.join("index.json");
                if index.is_file() {
                    match self.read_session_file(&index) {
                        Ok(mut data) => {
                            let n = sanitize_tool_chains(&mut data.messages);
                            if n > 0 {
                                self.write_session_index(&data)?;
                                removed += n;
                                touched += 1;
                            }
                        }
                        Err(e) => log::warn!(
                            target: "session",
                            "跳过无法解析的会话索引 {}: {e:#}",
                            index.display()
                        ),
                    }
                }

                let raw_dir = dir.join("raw");
                if raw_dir.is_dir() {
                    for raw_entry in std::fs::read_dir(&raw_dir)
                        .with_context(|| format!("读取 raw 目录失败: {}", raw_dir.display()))?
                    {
                        let path = raw_entry.context("读取 raw 条目失败")?.path();
                        if path.extension().and_then(|s| s.to_str()) != Some("json") {
                            continue;
                        }
                        match self.cleanup_raw_file(&path) {
                            Ok(n) if n > 0 => {
                                removed += n;
                                touched += 1;
                            }
                            Ok(_) => {}
                            Err(e) => log::warn!(
                                target: "session",
                                "跳过无法清洗的 raw 文件 {}: {e:#}",
                                path.display()
                            ),
                        }
                    }
                }
            }
        }

        if removed > 0 {
            log::info!(
                target: "session",
                "旧会话工具链清理完成：删除 {removed} 条不完整/孤立消息，涉及 {touched} 个文件"
            );
        }

        std::fs::create_dir_all(self.session_path())
            .with_context(|| "创建会话目录失败")?;
        std::fs::write(&marker, format!("removed={removed}\n"))
            .with_context(|| format!("写入清洗标记失败: {}", marker.display()))?;
        Ok(removed)
    }

    /// 一次性把旧的分渠道 current 迁移到统一会话 `current/main.json`。
    ///
    /// 步骤：app → main（保留原 id）→ 其余 current 归档为 Archived →
    /// 合并未压缩消息（去重 + token 预算）→ 删除旧 current → 写标记。
    /// 任何文件失败都不写标记，下次启动重试；失败文件不删除。
    pub fn migrate_to_main_session(&self) -> Result<MigrationReport> {
        let root = self.session_path();
        let marker = root.join(UNIFIED_SESSION_MARKER);
        if marker.is_file() {
            return Ok(MigrationReport::default());
        }

        let current_dir = root.join("current");
        if !current_dir.is_dir() {
            std::fs::create_dir_all(&current_dir)
                .with_context(|| format!("创建 current 目录失败: {}", current_dir.display()))?;
        }

        let main_path = current_dir.join(format!("{ACTIVE_SESSION_KEY}.json"));
        let app_path = current_dir.join("app.json");
        let mut report = MigrationReport::default();
        let mut consumed_app: Option<PathBuf> = None;

        // ① app → main：保留旧 id，raw / 向量 chunk / artifact 都不失效
        if !main_path.is_file() && app_path.is_file() {
            match self.read_session_file(&app_path) {
                Ok(mut app) => {
                    app.status = SessionStatus::Active;
                    self.write_session_file(&main_path, &app)?;
                    self.write_session_index(&app)?;
                    consumed_app = Some(app_path.clone());
                }
                Err(e) => {
                    log::warn!(
                        target: "session",
                        "迁移：解析 app 会话失败，本次不删除任何旧 current: {e:#}"
                    );
                    report.failures += 1;
                }
            }
        }

        // 没有可迁移的 app 时，创建全新的 main 槽位
        if !main_path.is_file() {
            let fresh = SessionData::new_active(format!("{ACTIVE_SESSION_KEY}_{}", now_id_ts()));
            self.write_session_file(&main_path, &fresh)?;
            self.write_session_index(&fresh)?;
        }

        let mut main_data = self.read_session_file(&main_path)?;
        main_data.status = SessionStatus::Active;
        if main_data.id.is_empty() {
            anyhow::bail!("迁移失败：main 会话缺少 id");
        }
        let main_id = main_data.id.clone();

        // ② 归档其余旧 current，并收集未压缩消息
        let cfg = self.config.get();
        let budget =
            ContextBudget::from_config(cfg.active_model_config(), &cfg.session_memory).trigger_tokens;
        let mut merge: Vec<(String, Vec<Message>)> = Vec::new();
        let mut delete_paths: Vec<PathBuf> = consumed_app.into_iter().collect();

        for entry in std::fs::read_dir(&current_dir)
            .with_context(|| format!("读取 current 目录失败: {}", current_dir.display()))?
        {
            let path = entry.context("读取 current 条目失败")?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if path == main_path
                || delete_paths.iter().any(|p| p == &path)
            {
                continue;
            }

            let mut data = match self.read_session_file(&path) {
                Ok(data) => data,
                Err(e) => {
                    log::warn!(
                        target: "session",
                        "迁移：跳过无法解析的 session {}: {e:#}",
                        path.display()
                    );
                    report.failures += 1;
                    continue;
                }
            };
            if data.id.is_empty() {
                log::warn!(target: "session", "迁移：跳过缺少 id 的 session {}", path.display());
                report.failures += 1;
                continue;
            }

            let _ = sanitize_tool_chains(&mut data.messages);

            // 空会话不产生归档
            if data.messages.is_empty()
                && data.compressed.is_empty()
                && data.super_compressed.is_none()
            {
                match std::fs::remove_file(&path) {
                    Ok(_) => delete_paths.push(path),
                    Err(e) => {
                        log::warn!(
                            target: "session",
                            "迁移：删除空会话失败 {}: {e:#}",
                            path.display()
                        );
                        report.failures += 1;
                    }
                }
                continue;
            }

            data.status = SessionStatus::Archived;
            if let Err(e) = self.write_session_index(&data) {
                log::warn!(
                    target: "session",
                    "迁移：归档会话 {} 失败: {e:#}",
                    data.id
                );
                report.failures += 1;
                continue;
            }
            report.archived += 1;

            for group in group_turns(&data.messages) {
                let mut group = group;
                fill_group_origin(&mut group);
                for message in group.iter_mut() {
                    match message {
                        Message::User { meta: Some(origin), .. }
                        | Message::Assistant { origin: Some(origin), .. }
                        | Message::Tool { origin: Some(origin), .. } => {
                            normalize_legacy_trust(origin)
                        }
                        _ => {}
                    }
                }
                merge.push((data.id.clone(), group));
            }
            delete_paths.push(path);
        }

        // ③ 按轮次时间排序，从新到旧在预算内选入；main 原有消息始终保留
        merge.sort_by(|a, b| turn_time(&a.1).cmp(turn_time(&b.1)));
        let mut seen: HashSet<_> = main_data.messages.iter().map(dedup_key).collect();
        let main_tokens = estimate_messages_tokens(&build_history_messages(&main_data));
        let mut remaining = budget.saturating_sub(main_tokens);
        let mut selected_rev: Vec<(String, Vec<Message>)> = Vec::new();

        for (idx, (old_id, group)) in merge.into_iter().enumerate().rev() {
            let group: Vec<Message> = group
                .into_iter()
                .filter(|message| !seen.contains(&dedup_key(message)))
                .collect();
            if group.is_empty() {
                continue;
            }
            let cost = estimate_text_tokens(&build_transcript(&group));
            if cost > remaining {
                // 选满即停止，更旧的轮次完整留在归档会话里
                report.skipped_turns += idx + 1;
                break;
            }
            remaining -= cost;
            for message in &group {
                seen.insert(dedup_key(message));
            }
            selected_rev.push((old_id, group));
        }

        let selected: Vec<(String, Vec<Message>)> = selected_rev.into_iter().rev().collect();
        let mut merged_messages: Vec<Message> = Vec::new();
        for (old_id, group) in &selected {
            self.copy_artifacts_to(&main_id, old_id, group);
            merged_messages.extend(group.iter().cloned());
        }
        report.merged_messages = merged_messages.len();

        if !merged_messages.is_empty() {
            merged_messages.extend(std::mem::take(&mut main_data.messages));
            main_data.messages = merged_messages;
            // 旧轮次的时间早于 main 原消息，锚点已失效，交给 provider 重新锚定
            main_data.stats = SessionStats::default();
        }
        self.write_session_file(&main_path, &main_data)?;

        // ④ 归档与合并都成功后删除旧 current
        for path in delete_paths {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!(
                    target: "session",
                    "迁移：删除旧 current 失败 {}: {e:#}",
                    path.display()
                );
                report.failures += 1;
            }
        }

        if report.is_clean() {
            std::fs::write(
                &marker,
                format!(
                    "archived={}\nmerged={}\nskipped={}\n",
                    report.archived, report.merged_messages, report.skipped_turns
                ),
            )
            .with_context(|| format!("写入迁移标记失败: {}", marker.display()))?;
            log::info!(
                target: "session",
                "统一会话迁移完成：归档 {} 个旧会话，合并 {} 条消息，跳过 {} 个超预算轮次",
                report.archived,
                report.merged_messages,
                report.skipped_turns
            );
        } else {
            log::warn!(
                target: "session",
                "统一会话迁移未完全成功（{} 个文件失败），下次启动将重试",
                report.failures
            );
        }

        Ok(report)
    }

    /// 把合并轮次引用的 artifact 从旧会话目录复制到 main，已存在则跳过
    fn copy_artifacts_to(&self, main_id: &str, old_id: &str, messages: &[Message]) {
        if main_id == old_id {
            return;
        }
        let src = self.session_path().join("artifacts").join(old_id);
        let dst = self.session_path().join("artifacts").join(main_id);
        for message in messages {
            let Message::Tool {
                artifact: Some(artifact),
                ..
            } = message
            else {
                continue;
            };
            let from = src.join(format!("{}.txt", artifact.id));
            if !from.is_file() {
                continue;
            }
            let to = dst.join(format!("{}.txt", artifact.id));
            if to.exists() {
                log::warn!(
                    target: "session",
                    "迁移：artifact {} 已存在于 main，跳过复制",
                    artifact.id
                );
                continue;
            }
            if let Some(parent) = to.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::warn!(target: "session", "迁移：创建 artifact 目录失败: {e:#}");
                    continue;
                }
            }
            if let Err(e) = std::fs::copy(&from, &to) {
                log::warn!(
                    target: "session",
                    "迁移：复制 artifact {} 失败: {e:#}",
                    from.display()
                );
            }
        }
    }

    /// 清洗单个 raw 块：兼容信封结构与旧版裸数组，保留其余字段。
    fn cleanup_raw_file(&self, path: &Path) -> Result<usize> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取 raw 文件失败: {}", path.display()))?;
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("解析 raw 文件失败: {}", path.display()))?;

        let mut messages: Vec<Message> = if value.is_array() {
            serde_json::from_value(value.clone())
                .with_context(|| format!("解析 raw 消息失败: {}", path.display()))?
        } else {
            let Some(items) = value.get("messages") else {
                return Ok(0);
            };
            serde_json::from_value(items.clone())
                .with_context(|| format!("解析 raw 消息失败: {}", path.display()))?
        };

        let removed = sanitize_tool_chains(&mut messages);
        if removed == 0 {
            return Ok(0);
        }

        let serialized = serde_json::to_value(&messages).context("序列化 raw 消息失败")?;
        if value.is_array() {
            value = serialized;
        } else {
            value["messages"] = serialized;
        }
        let text = serde_json::to_string_pretty(&value).context("序列化 raw 文件失败")?;
        let tmp = temp_path(path);
        std::fs::write(&tmp, text)
            .with_context(|| format!("写入 raw 临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("替换 raw 文件失败: {}", path.display()))?;
        Ok(removed)
    }
}

/// 一轮消息的排序时间（取首条消息的 time）
fn turn_time(group: &[Message]) -> &str {
    match group.first() {
        Some(Message::User { time, .. })
        | Some(Message::Assistant { time, .. })
        | Some(Message::Tool { time, .. }) => time,
        None => "",
    }
}
