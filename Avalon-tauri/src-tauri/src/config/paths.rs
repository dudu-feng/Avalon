// 配置定位与派生路径
//
// 负责回答两件事：
//   1. Avalon-config.toml 在哪里（locate_config）
//   2. 配置驱动的 data / file 根目录下子路径怎么推导（impl AppConfig）

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::types::AppConfig;

/// 定位 Avalon-config.toml，优先级：
///   1. 环境变量 AVALON_CONFIG_PATH —— 直接指定完整路径
///   2. 开发态：CARGO_MANIFEST_DIR 下的 Avalon-config.toml（src-tauri 内）
///   3. 发布态：可执行文件同级的 Avalon-config.toml
pub fn locate_config() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("AVALON_CONFIG_PATH") {
        let p = p.trim();
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }

    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        return Ok(PathBuf::from(m).join("Avalon-config.toml"));
    }

    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("Avalon-config.toml")))
        .with_context(|| "无法定位 Avalon-config.toml（请设置 AVALON_CONFIG_PATH）".to_string())
}

/// 推导项目根：
///   开发态：CARGO_MANIFEST_DIR = {root}/Avalon-tauri/src-tauri → root
///   发布态：可执行文件所在目录
#[allow(dead_code)] // 仅被派生路径方法调用，后续模块接入后自然消除
fn default_project_root() -> PathBuf {
    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest = PathBuf::from(m);
        if let Some(root) = manifest.parent().and_then(|p| p.parent()) {
            return root.to_path_buf();
        }
    }

    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[allow(dead_code)] // 派生路径方法供后续模块（llm/session/vector）调用
impl AppConfig {
    /// 实际 data 根：配置项为空时按约定推导 project_root/data
    pub fn data_root(&self) -> PathBuf {
        if self.paths.data_root.as_os_str().is_empty() {
            default_project_root().join("data")
        } else {
            self.paths.data_root.clone()
        }
    }

    /// 实际 file 根：配置项为空时按约定推导 project_root/file
    pub fn file_root(&self) -> PathBuf {
        if self.paths.file_root.as_os_str().is_empty() {
            default_project_root().join("file")
        } else {
            self.paths.file_root.clone()
        }
    }

    // —— data 内固定子目录（对齐 Python 端 data 目录结构）——

    pub fn prompt_file_path(&self) -> PathBuf {
        self.data_root().join("memory/prompt")
    }

    pub fn memory_path(&self) -> PathBuf {
        self.data_root().join("memory")
    }

    pub fn session_path(&self) -> PathBuf {
        self.data_root().join("memory/session")
    }

    pub fn session_index_path(&self) -> PathBuf {
        self.data_root().join("memory/session/index.json")
    }

    pub fn vector_db_path(&self) -> PathBuf {
        self.data_root().join("vector/vector_db")
    }

    pub fn model_cache_dir(&self) -> PathBuf {
        self.data_root().join("vector/models/embedding")
    }

    pub fn whisper_model_path(&self) -> PathBuf {
        self.data_root().join("models/whisper")
    }

    // —— 二级派生 ——

    /// 本地 embedding 模型完整路径 = model_cache_dir + local_model
    pub fn local_embedding_model_path(&self) -> PathBuf {
        self.model_cache_dir().join(&self.embedding.local_model)
    }

    pub fn zvec_db_path(&self) -> PathBuf {
        self.vector_db_path().join("zvec")
    }

    pub fn chroma_db_path(&self) -> PathBuf {
        self.vector_db_path().join("chroma")
    }

    // —— file 相关 ——

    pub fn file_path(&self) -> PathBuf {
        self.file_root()
    }

    pub fn temp_file_path(&self) -> PathBuf {
        self.file_root().join("temp")
    }
}
