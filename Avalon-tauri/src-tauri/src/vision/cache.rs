// 视觉缓存
//
// 缓存键 = 原图 sha + 预处理参数 + 模型 + 提示词版本。
// 缓存文件都在 file/cache 下，可随时清空后重建。

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::VisionConfig;

/// 临时文件序号：同进程内并发生成同一张图的缓存时，避免临时文件名互相覆盖
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 感知哈希索引文件名（位于 `cache/vlm/`）
pub const PHASH_INDEX_FILE: &str = "phash-index.json";

/// 近重复判定的最大 Hamming 距离。
///
/// dHash 是低频指纹，同布局不同文字的两张截图可能只有几位差异；为了不复用
/// 错误的观察，默认要求**完全一致**。重编码/重传通常会得到相同的 dHash，
/// 因此仍能覆盖最常见的情况。
pub const NEAR_DUPLICATE_MAX_DISTANCE: u32 = 0;

/// 允许的宽高比误差：同布局但裁切比例不同的图不参与复用
const ASPECT_TOLERANCE: f64 = 0.01;

/// 纯色/极低纹理图的 dHash 全为 0 或接近全 0，不能用于近重复判断
const MIN_TEXTURE_BITS: u32 = 6;
const MAX_TEXTURE_BITS: u32 = 58;

/// 感知哈希索引条目：只保存「指纹 → 缓存文件」的映射，不保存图片内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhashEntry {
    pub phash: String,
    /// 宽 / 高，用于排除比例不同的图
    pub aspect: f64,
    /// `cache/vlm/` 下的文件名（不含目录）
    pub file: String,
    pub model: String,
    pub prompt_version: u32,
    pub updated_at: String,
}

/// 读取索引，同时丢弃缓存文件已不存在的条目。
/// 文件缺失或损坏都当作空索引，下一次写入会重建
pub fn read_phash_index(dir: &Path) -> Vec<PhashEntry> {
    let entries: Vec<PhashEntry> = read_json(&dir.join(PHASH_INDEX_FILE)).unwrap_or_default();
    entries
        .into_iter()
        .filter(|e| dir.join(&e.file).is_file())
        .collect()
}

pub fn write_phash_index(dir: &Path, entries: &[PhashEntry]) -> Result<()> {
    // write_json 的泛型参数要求 Sized，切片需先转成 Vec
    write_json(&dir.join(PHASH_INDEX_FILE), &entries.to_vec())
}

/// 两个 16 位十六进制 dHash 的 Hamming 距离；格式非法返回 None
pub fn phash_distance(a: &str, b: &str) -> Option<u32> {
    let a = u64::from_str_radix(a, 16).ok()?;
    let b = u64::from_str_radix(b, 16).ok()?;
    Some((a ^ b).count_ones())
}

/// 纹理是否足够丰富，能支撑近重复判断
pub fn is_textured(phash: &str) -> bool {
    match u64::from_str_radix(phash, 16) {
        Ok(bits) => (MIN_TEXTURE_BITS..=MAX_TEXTURE_BITS).contains(&bits.count_ones()),
        Err(_) => false,
    }
}

/// 在同模型、同提示词版本、同宽高比的条目里找最近的近重复
pub fn find_similar<'a>(
    entries: &'a [PhashEntry],
    phash: &str,
    model: &str,
    prompt_version: u32,
    aspect: f64,
    max_distance: u32,
) -> Option<&'a PhashEntry> {
    entries
        .iter()
        .filter(|e| e.model == model && e.prompt_version == prompt_version)
        .filter(|e| (e.aspect - aspect).abs() <= ASPECT_TOLERANCE)
        .filter_map(|e| phash_distance(&e.phash, phash).map(|d| (d, e)))
        .filter(|(d, _)| *d <= max_distance)
        .min_by_key(|(d, _)| *d)
        .map(|(_, e)| e)
}

/// 按缓存文件名 upsert：同一观察被重新生成时替换而不是追加
pub fn upsert_phash(entries: &mut Vec<PhashEntry>, entry: PhashEntry) {
    if let Some(existing) = entries.iter_mut().find(|e| e.file == entry.file) {
        *existing = entry;
    } else {
        entries.push(entry);
    }
}

/// 参数哈希只取前 8 位十六进制：缓存目录里同图不同参数的分支通常很少，
/// 文件名保持可读比极致防碰撞更重要；完整参数仍会写进缓存内容
pub fn param_key(model: &str, cfg: &VisionConfig, prompt_version: u32) -> String {
    let raw = format!(
        "{model}|{}|{}|{}|{}|p{prompt_version}",
        cfg.max_edge, cfg.max_bytes, cfg.jpeg_quality, cfg.detail
    );
    let digest = Sha256::digest(raw.as_bytes());
    hex::encode(digest)[..8].to_string()
}

/// 缓存文件名：`<sha>-<param_key>.json`
pub fn file_name(sha256: &str, param_key: &str) -> String {
    format!("{sha256}-{param_key}.json")
}

/// 读取 JSON；文件不存在或内容损坏都当作未命中
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 原子写 JSON：临时文件 + rename，避免半截缓存被读到
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("序列化缓存失败")?;
    write_bytes(path, &bytes)
}

/// 原子写字节
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("缓存路径没有父目录: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("创建缓存目录失败: {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .with_context(|| format!("非法缓存文件名: {}", path.display()))?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(
        ".{file_name}.{}.{seq}.tmp",
        std::process::id()
    ));

    let mut file = fs::File::create(&tmp)
        .with_context(|| format!("创建缓存临时文件失败: {}", tmp.display()))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("写入缓存临时文件失败: {}", tmp.display()));
    }
    drop(file);

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("缓存原子替换失败: {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> VisionConfig {
        VisionConfig::default()
    }

    #[test]
    fn same_inputs_hit_same_key() {
        let a = param_key("vision-model", &cfg(), 1);
        let b = param_key("vision-model", &cfg(), 1);
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn model_or_prompt_change_invalidates_key() {
        let base = param_key("model-a", &cfg(), 1);
        assert_ne!(base, param_key("model-b", &cfg(), 1));
        assert_ne!(base, param_key("model-a", &cfg(), 2));
    }

    #[test]
    fn cache_name_contains_sha_and_key() {
        let key = param_key("m", &cfg(), 1);
        let name = file_name("abc123", &key);
        assert!(name.starts_with("abc123-"));
        assert!(name.ends_with(".json"));
    }

    fn entry(file: &str, phash: &str, model: &str, version: u32) -> PhashEntry {
        PhashEntry {
            phash: phash.to_string(),
            aspect: 16.0 / 9.0,
            file: file.to_string(),
            model: model.to_string(),
            prompt_version: version,
            updated_at: String::new(),
        }
    }

    #[test]
    fn phash_distance_counts_bits() {
        assert_eq!(phash_distance("0000000000000000", "0000000000000000"), Some(0));
        assert_eq!(phash_distance("0000000000000000", "0000000000000001"), Some(1));
        assert_eq!(phash_distance("0000000000000000", "ffffffffffffffff"), Some(64));
        assert_eq!(phash_distance("xyz", "0000000000000000"), None);
    }

    #[test]
    fn flat_phash_is_not_textured() {
        assert!(!is_textured("0000000000000000"));
        assert!(!is_textured("ffffffffffffffff"));
        assert!(is_textured("5f3c9a0e7b21d448"));
    }

    #[test]
    fn finds_only_exact_match_with_same_model_version_and_aspect() {
        let entries = vec![
            entry("a.json", "0000000000000000", "m1", 1),
            entry("b.json", "0000000000000001", "m1", 1),
            entry("c.json", "0000000000000000", "m2", 1),
            entry("d.json", "0000000000000000", "m1", 2),
        ];

        let hit = find_similar(&entries, "0000000000000000", "m1", 1, 16.0 / 9.0, 0).unwrap();
        assert_eq!(hit.file, "a.json");
        assert!(
            find_similar(&entries, "0000000000000002", "m1", 1, 16.0 / 9.0, 0).is_none(),
            "默认阈值要求完全一致"
        );
        assert!(
            find_similar(&entries, "0000000000000000", "m1", 1, 4.0 / 3.0, 0).is_none(),
            "宽高比不同不应复用"
        );
        assert!(find_similar(&entries, "0000000000000000", "m3", 1, 16.0 / 9.0, 0).is_none());
    }

    #[test]
    fn upsert_replaces_same_file() {
        let mut entries = vec![entry("a.json", "0000000000000001", "m", 1)];
        upsert_phash(&mut entries, entry("a.json", "0000000000000002", "m", 1));
        upsert_phash(&mut entries, entry("b.json", "0000000000000003", "m", 1));

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].phash, "0000000000000002");
    }

    #[test]
    fn phash_index_roundtrip_prunes_missing_files() {
        let dir = std::env::temp_dir().join(format!(
            "avalon_phash_index_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), b"{}").unwrap();

        let entries = vec![
            entry("a.json", "0000000000000001", "m", 1),
            entry("gone.json", "0000000000000002", "m", 1),
        ];
        write_phash_index(&dir, &entries).unwrap();

        let loaded = read_phash_index(&dir);
        assert_eq!(loaded.len(), 1, "缓存文件已不存在的条目应被丢弃");
        assert_eq!(loaded[0].file, "a.json");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
