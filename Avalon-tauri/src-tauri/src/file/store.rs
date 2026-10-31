// 文件落盘核心
//
// 职责：命名、内容嗅探、哈希、原子写、sidecar、路径越界校验、过期清理。
// 目录布局与规则见 doc/v0.7/02-file-storage.md。

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate};
use sha2::{Digest, Sha256};

use super::types::{
    CleanupReport, FileKind, FileSource, Sidecar, StoredFile, SIDECAR_SCHEMA,
};
use crate::config::AppConfig;

/// 单文件上限。再大就不适合走 base64 + 全量内存读取
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// temp 默认保留时长
pub const TEMP_TTL_HOURS: i64 = 24;

/// quarantine 默认保留天数
pub const QUARANTINE_TTL_DAYS: i64 = 7;

/// 保存请求。参数较多，用结构体显式携带来源与归属
pub struct SaveRequest<'a> {
    pub bytes: &'a [u8],
    pub original_name: &'a str,
    pub declared_mime: Option<&'a str>,
    pub source: FileSource,
    pub session_id: Option<&'a str>,
    pub message_id: Option<&'a str>,
}

/// 以 `file_root` 为边界的文件存储
pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn from_config(cfg: &AppConfig) -> Self {
        Self::new(cfg.file_root())
    }

    /// 存储根。测试与后续存储用量统计使用
    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 保存普通文件。图片会按类型自动落进 `image/` 子目录
    pub fn save(&self, req: SaveRequest<'_>) -> Result<StoredFile> {
        self.save_inner(req, false, Local::now())
    }

    /// 保存图片：强制要求内容确为图片，否则隔离并拒绝。
    /// 飞书图片下载接入后使用 —— 那里必须先确认是图片再落盘
    #[allow(dead_code)]
    pub fn save_image(&self, req: SaveRequest<'_>) -> Result<StoredFile> {
        self.save_inner(req, true, Local::now())
    }

    /// 解析相对路径并做越界校验。
    ///
    /// 先逐段检查组件，再 canonicalize 比对前缀：
    /// 只做后者的话，文件不存在时会先报 IO 错误，错误信息对调用方没有意义
    pub fn resolve(&self, relative: &str) -> Result<PathBuf> {
        let relative = relative.trim();
        if relative.is_empty() {
            bail!("路径为空");
        }

        let rel = Path::new(relative);
        if rel.is_absolute() {
            bail!("拒绝绝对路径: {relative}");
        }
        for comp in rel.components() {
            if !matches!(comp, Component::Normal(_)) {
                bail!("路径包含非法片段: {relative}");
            }
        }

        let root = self
            .root
            .canonicalize()
            .with_context(|| format!("文件根不存在: {}", self.root.display()))?;
        let candidate = root.join(rel);
        let canon = candidate
            .canonicalize()
            .with_context(|| format!("文件不存在: {relative}"))?;
        if !canon.starts_with(&root) {
            bail!("路径越界: {relative}");
        }
        Ok(canon)
    }

    /// 用默认 TTL 清理 temp / quarantine。定时清理任务接入后使用
    #[allow(dead_code)]
    pub fn cleanup(&self) -> Result<CleanupReport> {
        self.cleanup_with_ttl(Local::now(), TEMP_TTL_HOURS, QUARANTINE_TTL_DAYS)
    }

    pub fn cleanup_with_ttl(
        &self,
        now: DateTime<Local>,
        temp_ttl_hours: i64,
        quarantine_ttl_days: i64,
    ) -> Result<CleanupReport> {
        if temp_ttl_hours < 0 || quarantine_ttl_days < 0 {
            bail!("TTL 不能为负数");
        }

        let temp_cutoff = now.clone() - Duration::hours(temp_ttl_hours);
        let quarantine_cutoff = now - Duration::days(quarantine_ttl_days);

        let mut report = CleanupReport::default();
        self.cleanup_dir(&self.temp_dir(), temp_cutoff, &mut report)?;
        self.cleanup_dir(&self.quarantine_dir(), quarantine_cutoff, &mut report)?;
        Ok(report)
    }

    fn save_inner(
        &self,
        req: SaveRequest<'_>,
        force_image: bool,
        now: DateTime<Local>,
    ) -> Result<StoredFile> {
        if req.bytes.is_empty() {
            bail!("文件内容为空");
        }
        let size = req.bytes.len() as u64;
        if size > MAX_FILE_BYTES {
            bail!(
                "文件超过上限 {} MB",
                MAX_FILE_BYTES / 1024 / 1024
            );
        }

        let declared = req.declared_mime.unwrap_or("");
        // detected 为空表示内容嗅探失败，此时才退回声明 MIME；
        // 图片校验必须看 detected —— 声明值是不可信输入
        let detected = detect_mime(req.bytes);
        let mime = detected
            .clone()
            .unwrap_or_else(|| fallback_mime(req.bytes, declared));
        let original_ext = extension_of(req.original_name);

        // 可执行文件不落正式目录：扩展名声明或内容嗅探命中都算
        if is_executable(&mime, original_ext.as_deref()) {
            self.quarantine(req.bytes, &req, &mime, "可执行文件", now)?;
            bail!("拒绝保存可执行文件: {}", req.original_name);
        }

        if force_image
            && !detected
                .as_deref()
                .is_some_and(|m| m.starts_with("image/"))
        {
            self.quarantine(
                req.bytes,
                &req,
                &mime,
                "声明为图片但内容不是有效图片",
                now,
            )?;
            bail!("内容不是有效图片: {}", req.original_name);
        }

        let kind = FileKind::from_mime(&mime);
        let sha256 = sha256_hex(req.bytes);
        let date = format_date(now.date_naive());

        // 图片进 image/<date>，普通文件进 <date>
        let rel_dir = if kind.is_image() {
            PathBuf::from("image").join(&date)
        } else {
            PathBuf::from(&date)
        };

        let timestamp = now.format("%Y%m%d-%H%M%S").to_string();
        let base = format!("{}-{}-{}", req.source.prefix(), timestamp, &sha256[..8]);
        let ext = extension_for_mime(&mime)
            .or(original_ext)
            .unwrap_or_else(|| "bin".to_string());

        let (rel_path, id) = self.unique_path(&rel_dir, &base, &ext)?;
        let abs_path = self.root.join(&rel_path);
        write_atomic(&abs_path, req.bytes)?;

        let original_name = non_empty(req.original_name).map(str::to_string);
        let sidecar = Sidecar {
            schema: SIDECAR_SCHEMA,
            id: id.clone(),
            kind,
            source: req.source,
            original_name: original_name.clone(),
            mime: mime.clone(),
            ext: ext.clone(),
            size,
            sha256: sha256.clone(),
            created_at: now.to_rfc3339(),
            session_id: req.session_id.and_then(non_empty).map(str::to_string),
            message_id: req.message_id.and_then(non_empty).map(str::to_string),
            // 图片尺寸/哈希由预处理流水线后续补齐
            image: None,
            derived: None,
        };

        let meta_name = format!("{id}.meta.json");
        let rel_meta = rel_dir.join(&meta_name);
        let json = serde_json::to_vec_pretty(&sidecar).context("序列化 sidecar 失败")?;

        // sidecar 写失败就把主文件撤掉，避免留下无法追溯来源的孤儿文件
        if let Err(e) = write_atomic(&self.root.join(&rel_meta), &json) {
            let _ = fs::remove_file(&abs_path);
            return Err(e);
        }

        Ok(StoredFile {
            id,
            path: relative_string(&rel_path),
            kind,
            mime,
            size,
            sha256,
            created_at: now.to_rfc3339(),
            original_name,
            meta_path: Some(relative_string(&rel_meta)),
        })
    }

    /// 生成不冲突的文件名。同时避开同名 sidecar，避免主文件与元数据互相覆盖
    fn unique_path(
        &self,
        rel_dir: &Path,
        base: &str,
        ext: &str,
    ) -> Result<(PathBuf, String)> {
        for n in 1..=9999u32 {
            let id = if n == 1 {
                base.to_string()
            } else {
                format!("{base}-{n}")
            };
            let rel = rel_dir.join(format!("{id}.{ext}"));
            let meta = rel_dir.join(format!("{id}.meta.json"));
            if !self.root.join(&rel).exists() && !self.root.join(&meta).exists() {
                return Ok((rel, id));
            }
        }
        bail!("同名文件过多，无法生成唯一文件名: {base}")
    }

    /// 拒绝的文件不直接删除，写进 quarantine 并留下原因
    fn quarantine(
        &self,
        bytes: &[u8],
        req: &SaveRequest<'_>,
        mime: &str,
        reason: &str,
        now: DateTime<Local>,
    ) -> Result<()> {
        let dir = self.quarantine_dir().join(format_date(now.date_naive()));
        fs::create_dir_all(&dir)
            .with_context(|| format!("创建隔离目录失败: {}", dir.display()))?;

        let sha256 = sha256_hex(bytes);
        let stem = format!(
            "rejected-{}-{}",
            now.format("%Y%m%d-%H%M%S"),
            &sha256[..8]
        );

        let (bin_path, reason_path) = unique_quarantine_paths(&dir, &stem);
        write_atomic(&bin_path, bytes)?;

        let note = format!(
            "reason: {reason}\noriginal_name: {}\ndeclared_mime: {}\ndetected_mime: {mime}\nsize: {}\nsha256: {sha256}\ncreated_at: {}\n",
            req.original_name,
            req.declared_mime.unwrap_or(""),
            bytes.len(),
            now.to_rfc3339(),
        );
        write_atomic(&reason_path, note.as_bytes())?;
        Ok(())
    }

    /// 递归清理：只删过期文件与清空后的子目录，不跟随符号链接
    fn cleanup_dir(
        &self,
        dir: &Path,
        cutoff: DateTime<Local>,
        report: &mut CleanupReport,
    ) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        let entries = fs::read_dir(dir)
            .with_context(|| format!("读取目录失败: {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();

            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                self.cleanup_dir(&path, cutoff, report)?;
                // 清理后变空的子目录一并删掉；temp 与 quarantine 根自身保留
                if fs::read_dir(&path)?.next().is_none() && fs::remove_dir(&path).is_ok() {
                    report.removed_dirs += 1;
                }
            } else if file_type.is_file() {
                let meta = entry.metadata()?;
                let modified: DateTime<Local> = meta.modified()?.into();
                if modified < cutoff {
                    report.freed_bytes += meta.len();
                    fs::remove_file(&path)
                        .with_context(|| format!("删除过期文件失败: {}", path.display()))?;
                    report.removed_files += 1;
                }
            }
        }
        Ok(())
    }

    fn temp_dir(&self) -> PathBuf {
        self.root.join("temp")
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.join("quarantine")
    }
}

/// 内容嗅探。返回 None 表示无法从文件头判断类型
fn detect_mime(bytes: &[u8]) -> Option<String> {
    infer::get(bytes).map(|kind| kind.mime_type().to_string())
}

/// 嗅探失败时的兜底：声明 MIME → 文本特征 → octet-stream。
/// 纯文本文件（csv/md/log）infer 通常识别不了，声明值反而是最可靠的线索
fn fallback_mime(bytes: &[u8], declared: &str) -> String {
    let declared = declared
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !declared.is_empty() {
        return declared;
    }

    if looks_like_text(bytes) {
        return "text/plain".to_string();
    }
    "application/octet-stream".to_string()
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let end = bytes.len().min(8192);
    let sample = &bytes[..end];
    !sample.contains(&0) && std::str::from_utf8(sample).is_ok()
}

/// 可执行/脚本类：作为聊天附件上传没有正当用途，一律隔离
fn is_executable(mime: &str, ext: Option<&str>) -> bool {
    const EXECUTABLE_EXTS: &[&str] = &[
        "exe", "bat", "cmd", "ps1", "com", "scr", "msi", "dll", "vbs", "vbe", "wsf", "wsh",
        "hta", "lnk", "reg",
    ];
    if ext.is_some_and(|e| EXECUTABLE_EXTS.contains(&e)) {
        return true;
    }

    matches!(
        mime,
        "application/x-msdownload"
            | "application/x-msdos-program"
            | "application/vnd.microsoft.portable-executable"
            | "application/x-dosexec"
            | "application/x-executable"
            | "application/x-sh"
            | "application/x-bat"
            | "text/x-msdos-batch"
            | "application/x-powershell"
    )
}

fn extension_for_mime(mime: &str) -> Option<String> {
    mime_guess::get_mime_extensions_str(mime)?
        .first()
        .map(|ext| ext.to_string())
}

/// 从原文件名取扩展名。只接受短 ASCII 字母数字，
/// 其余（空、超长、含奇怪字符）一律忽略，交给 MIME 推导兜底
fn extension_of(name: &str) -> Option<String> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    if ext.is_empty() || ext.len() > 10 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext)
}

fn unique_quarantine_paths(dir: &Path, stem: &str) -> (PathBuf, PathBuf) {
    for n in 1..=9999u32 {
        let name = if n == 1 {
            stem.to_string()
        } else {
            format!("{stem}-{n}")
        };
        let bin = dir.join(format!("{name}.bin"));
        let reason = dir.join(format!("{name}.reason.txt"));
        if !bin.exists() && !reason.exists() {
            return (bin, reason);
        }
    }
    // 极端情况下直接覆盖最后一组，保证调用方总能拿到路径；
    // 隔离区本身是丢弃区，覆盖不构成数据风险
    (
        dir.join(format!("{stem}-9999.bin")),
        dir.join(format!("{stem}-9999.reason.txt")),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn relative_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

/// 同目录临时文件 + rename，避免半截文件被会话引用
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("路径没有父目录: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建目录失败: {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .with_context(|| format!("非法文件名: {}", path.display()))?;
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));

    let mut file = fs::File::create(&tmp)
        .with_context(|| format!("创建临时文件失败: {}", tmp.display()))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("写入临时文件失败: {}", tmp.display()));
    }
    drop(file);

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("原子替换失败: {}", path.display()));
    }
    Ok(())
}
