// 工具结果 artifact
//
// 大体积工具输出不再整段塞进会话历史：会话消息里只保留预览，全文落在
// data/memory/session/artifacts/{session_id}/{artifact_id}.txt，
// 模型需要时通过 read_tool_result 按字符偏移回读。
//
// 安全边界：session_id 与 artifact_id 都只允许 [A-Za-z0-9_-]，
// 路径完全由这两个白名单片段拼出，不接收任何外部路径。
// 不合法的 tool_call_id 会先哈希成安全文件名，避免路径穿越。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::types::ArtifactRef;

/// 单次回读的默认字符数
pub const DEFAULT_READ_LIMIT: usize = 4_000;
/// 单次回读的硬上限，避免模型一次拉回超大结果
pub const MAX_READ_LIMIT: usize = 20_000;

/// 路径片段最大长度
const MAX_ID_LEN: usize = 128;

/// 预览参数（对齐设计文档 5.2.2）
const PREVIEW_HEAD_LINES: usize = 30;
const PREVIEW_HEAD_CHARS: usize = 1_200;
const PREVIEW_TAIL_LINES: usize = 10;
const PREVIEW_TAIL_CHARS: usize = 300;

/// artifact 正文的一次读取结果
#[derive(Debug, Clone)]
pub struct ArtifactRead {
    pub id: String,
    pub content: String,
    /// 本次读取的起始字符偏移
    pub offset: usize,
    /// 全文总字符数
    pub total_chars: usize,
    /// 下一次读取应使用的 offset（等于本次结束位置）
    pub next_offset: usize,
}

/// 文件名安全：仅 ASCII 字母数字、下划线、连字符
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 把任意 tool_call_id 转成安全文件名；不合法时用哈希兜底。
pub fn safe_artifact_id(raw: &str) -> String {
    if is_safe_id(raw) {
        return raw.to_string();
    }
    let digest = Sha256::digest(raw.as_bytes());
    format!("h_{}", hex::encode(&digest[..8]))
}

/// artifacts 根目录：{session_path}/artifacts
fn artifacts_dir(root: &Path) -> PathBuf {
    root.join("artifacts")
}

/// 单个会话的 artifact 目录；路径片段不合法时直接拒绝
fn session_dir(root: &Path, session_id: &str) -> Result<PathBuf> {
    if !is_safe_id(session_id) {
        anyhow::bail!("会话 id 不合法，无法定位工具结果");
    }
    Ok(artifacts_dir(root).join(session_id))
}

fn artifact_path(root: &Path, session_id: &str, artifact_id: &str) -> Result<PathBuf> {
    if !is_safe_id(artifact_id) {
        anyhow::bail!("artifact id 不合法");
    }
    Ok(session_dir(root, session_id)?.join(format!("{artifact_id}.txt")))
}

/// 写入全文 artifact（同一 id 覆盖），返回引用信息
pub fn write(
    root: &Path,
    session_id: &str,
    artifact_id: &str,
    content: &str,
) -> Result<ArtifactRef> {
    let path = artifact_path(root, session_id, artifact_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建工具结果目录失败: {}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("写入工具结果失败: {}", path.display()))?;
    Ok(ArtifactRef {
        id: artifact_id.to_string(),
        chars: content.chars().count(),
        bytes: content.len(),
    })
}

/// 按字符偏移读取 artifact 正文
pub fn read(
    root: &Path,
    session_id: &str,
    artifact_id: &str,
    offset: usize,
    limit: usize,
) -> Result<ArtifactRead> {
    let path = artifact_path(root, session_id, artifact_id)?;
    let content = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "artifact:{} 不存在（只能读取当前会话范围内写入的工具结果）",
            artifact_id
        )
    })?;

    let total_chars = content.chars().count();
    let offset = offset.min(total_chars);
    let limit = limit.clamp(1, MAX_READ_LIMIT);
    let slice: String = content.chars().skip(offset).take(limit).collect();
    let next_offset = offset + slice.chars().count();

    Ok(ArtifactRead {
        id: artifact_id.to_string(),
        content: slice,
        offset,
        total_chars,
        next_offset,
    })
}

/// 生成预览（开头 + 结尾）。内容短到预览即全文时返回 None，不建 artifact。
pub fn build_preview(artifact_id: &str, content: &str) -> Option<String> {
    let head = take_head(content, PREVIEW_HEAD_LINES, PREVIEW_HEAD_CHARS);
    let tail = take_tail(content, PREVIEW_TAIL_LINES, PREVIEW_TAIL_CHARS);
    if head.len() >= content.len() - tail.len() {
        return None;
    }

    let total_chars = content.chars().count();
    let shown_chars = head.chars().count() + tail.chars().count();
    let truncated = total_chars.saturating_sub(shown_chars);

    Some(format!(
        "{head}\n\n... [已截断 {truncated} 字]\n\n{tail}\n\n[完整结果: artifact:{artifact_id}]"
    ))
}

/// 取前 N 行、最多 N 字符（按 UTF-8 边界截断）
fn take_head(content: &str, max_lines: usize, max_chars: usize) -> &str {
    let mut chars = 0usize;
    let mut lines = 0usize;
    for (idx, ch) in content.char_indices() {
        if chars >= max_chars || lines >= max_lines {
            return &content[..idx];
        }
        if ch == '\n' {
            lines += 1;
        }
        chars += 1;
    }
    content
}

/// 取后 N 行、最多 N 字符（按 UTF-8 边界截断）
fn take_tail(content: &str, max_lines: usize, max_chars: usize) -> &str {
    let mut chars = 0usize;
    let mut lines = 0usize;
    let mut start = content.len();
    for (idx, ch) in content.char_indices().rev() {
        if chars >= max_chars || lines >= max_lines {
            break;
        }
        if ch == '\n' {
            lines += 1;
        }
        chars += 1;
        start = idx;
    }
    &content[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "avalon-artifact-{tag}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[test]
    fn rejects_unsafe_ids() {
        assert!(is_safe_id("call_abc-123"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("../escape"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id("a\\b"));
        assert!(!is_safe_id("call.1"));
    }

    #[test]
    fn hashes_unsafe_tool_call_ids() {
        let id = safe_artifact_id("call:1/../x");
        assert!(is_safe_id(&id));
        assert!(id.starts_with("h_"));
        // 同一输入稳定
        assert_eq!(id, safe_artifact_id("call:1/../x"));
    }

    #[test]
    fn writes_and_reads_slices_by_chars() {
        let root = temp_root("read");
        let content: String = "一二三四五六七八九十".repeat(10);
        let r = write(&root, "desktop_2026", "call_1", &content).unwrap();
        assert_eq!(r.chars, 100);
        assert_eq!(r.bytes, content.len());

        let first = read(&root, "desktop_2026", "call_1", 0, 12).unwrap();
        assert_eq!(first.content.chars().count(), 12);
        assert_eq!(first.total_chars, 100);
        assert_eq!(first.next_offset, 12);

        let rest = read(&root, "desktop_2026", "call_1", first.next_offset, 999).unwrap();
        assert_eq!(rest.content.chars().count(), 88);
        assert_eq!(rest.next_offset, 100);

        let out_of_range = read(&root, "desktop_2026", "call_1", 500, 10).unwrap();
        assert!(out_of_range.content.is_empty());
        assert_eq!(out_of_range.offset, 100);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_is_scoped_to_session() {
        let root = temp_root("scope");
        write(&root, "session_a", "call_1", "内容").unwrap();
        assert!(read(&root, "session_b", "call_1", 0, 10).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn preview_keeps_head_and_tail() {
        let head = "头".repeat(PREVIEW_HEAD_CHARS);
        let middle = "中".repeat(10_000);
        let tail = "尾".repeat(PREVIEW_TAIL_CHARS);
        let content = format!("{head}\n{middle}\n{tail}");
        let preview = build_preview("call_x", &content).unwrap();

        assert!(preview.starts_with("头"));
        assert!(preview.contains("尾"));
        assert!(preview.contains("已截断"));
        assert!(preview.ends_with("[完整结果: artifact:call_x]"));
        assert!(preview.chars().count() < content.chars().count());
    }

    #[test]
    fn short_content_has_no_preview() {
        assert!(build_preview("call_x", "短内容").is_none());
        // 预览头尾能覆盖全文时也不需要 artifact
        assert!(build_preview("call_x", &"字".repeat(1_000)).is_none());
    }
}
