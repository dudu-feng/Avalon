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
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::config::VisionConfig;

/// 临时文件序号：同进程内并发生成同一张图的缓存时，避免临时文件名互相覆盖
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

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
}
