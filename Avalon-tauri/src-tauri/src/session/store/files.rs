// 会话文件 IO：存储布局、原子读写与 active 会话初始化。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use super::{FileSessionStore, ACTIVE_SESSION_KEY};
use crate::session::artifact;
use crate::session::now_id_ts;
use crate::session::types::{SessionData, SessionStatus};

/// 原子写临时文件名：进程内自增，避免同一目标的并发写共用临时文件互相截断。
pub(super) fn temp_path(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// 会话 ID 会直接拼进文件路径：仅允许文件名安全字符，拒绝 `..` / 分隔符 / 绝对路径。
pub(super) fn ensure_safe_session_id(id: &str) -> Result<()> {
    if artifact::is_safe_id(id) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("非法会话 ID: {id}"))
    }
}

impl FileSessionStore {
    // ============ 路径辅助 ============

    pub(super) fn session_path(&self) -> PathBuf {
        self.config.get().session_path()
    }

    /// 当前会话固定写入 current/main.json（全局唯一活跃会话槽位）。
    pub(super) fn current_file(&self) -> PathBuf {
        self.session_path()
            .join("current")
            .join(format!("{ACTIVE_SESSION_KEY}.json"))
    }

    pub(super) fn history_dir(&self) -> PathBuf {
        self.session_path().join("history")
    }

    // ============ 文件读写 ============

    /// 读当前会话；文件不存在返回空会话（inactive）
    pub(super) fn read_current(&self) -> Result<SessionData> {
        let path = self.current_file();
        if !path.exists() {
            return Ok(SessionData::empty());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("读取会话文件失败: {}", path.display()))?;
        // 旧格式（扁平 ChatMessage / 三段标记）不再兼容，解析失败降级空会话，避免阻断启动
        match serde_json::from_str(&content) {
            Ok(data) => Ok(data),
            Err(e) => {
                // 先备份再降级：后续 ensure_active 会用新会话覆盖 main.json，
                // 不备份的话损坏但可能可修复的原文件会被永久销毁。
                let backup = path.with_extension(format!("corrupt-{}", now_id_ts()));
                if let Err(re) = std::fs::rename(&path, &backup) {
                    log::warn!(
                        target: "session",
                        "会话文件解析失败且备份失败（{}）: {e}; {re}",
                        path.display()
                    );
                } else {
                    log::warn!(
                        target: "session",
                        "会话文件解析失败，已备份为 {}: {e}",
                        backup.display()
                    );
                }
                Ok(SessionData::empty())
            }
        }
    }

    /// 写当前会话（原子：tmp + rename，自动建目录）
    pub(super) fn write_current(&self, data: &SessionData) -> Result<()> {
        let path = self.current_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建会话目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(data).context("序列化会话数据失败")?;
        let tmp = temp_path(&path);
        std::fs::write(&tmp, &content)
            .with_context(|| format!("写入会话临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("原子替换会话文件失败: {}", path.display()))?;
        Ok(())
    }

    /// 确保会话就绪：active 则复用，否则新建（含 history 初始存档）
    pub(super) fn ensure_active(&self) -> Result<SessionData> {
        let data = self.read_current()?;
        if data.status == SessionStatus::Active && !data.id.is_empty() {
            return Ok(data);
        }
        self.create_active_session()
    }

    /// 新建一个 active 会话：写 current + 写 history/{id}/index.json（初始存档）
    pub(super) fn create_active_session(&self) -> Result<SessionData> {
        let fresh = SessionData::new_active(format!("{ACTIVE_SESSION_KEY}_{}", now_id_ts()));
        self.write_current(&fresh)?;
        self.write_session_index(&fresh)?;
        Ok(fresh)
    }

    /// 写会话 index 文件（history/{id}/index.json），自动建目录
    pub(super) fn write_session_index(&self, data: &SessionData) -> Result<()> {
        // id 可能来自磁盘上的 index.json（可被外部编辑），落盘前必须校验，
        // 否则绝对路径 / `..` 会把文件写到 history 目录之外。
        if !artifact::is_safe_id(&data.id) {
            anyhow::bail!("非法会话 ID，拒绝写入: {}", data.id);
        }
        let dir = self.history_dir().join(&data.id);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("创建会话目录失败: {}", dir.display()))?;
        self.write_session_file(&dir.join("index.json"), data)
    }

    /// 原子写任意会话文件（迁移与启动清洗按文件路径就地写回，避免全部落到 main）
    pub(super) fn write_session_file(&self, path: &Path, data: &SessionData) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建会话目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(data).context("序列化会话数据失败")?;
        let tmp = temp_path(path);
        std::fs::write(&tmp, &content)
            .with_context(|| format!("写入会话临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("原子替换会话文件失败: {}", path.display()))?;
        Ok(())
    }

    pub(super) fn read_session_file(&self, path: &Path) -> Result<SessionData> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取会话文件失败: {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("解析会话文件失败: {}", path.display()))
    }
}
