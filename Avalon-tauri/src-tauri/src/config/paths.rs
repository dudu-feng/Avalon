// 配置定位与派生路径
//
// 负责回答两件事：
//   1. Avalon-config.toml 在哪里（locate_config）
//   2. 配置驱动的 data / file 根目录下子路径怎么推导（impl AppConfig）

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::NaiveDate;

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
        // 开发态：配置放 src-tauri 上一层（Avalon-tauri/），避开 tauri dev 的 Rust watcher
        let manifest = PathBuf::from(m);
        let dir = manifest
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| manifest.clone());
        return Ok(dir.join("Avalon-config.toml"));
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

    /// 实际 file 根：配置项为空时按约定推导 data_root/file。
    ///
    /// 默认放在 data 下而不是与 data 平级，是为了备份/迁移时只需处理一个根目录，
    /// 不会漏掉上传文件与图片。
    pub fn file_root(&self) -> PathBuf {
        if self.paths.file_root.as_os_str().is_empty() {
            self.data_root().join("file")
        } else {
            self.paths.file_root.clone()
        }
    }

    // —— data 内固定子目录（对齐 Python 端 data 目录结构）——

    pub fn prompt_file_path(&self) -> PathBuf {
        self.data_root().join("memory/prompt")
    }

    /// 技能目录：data_root/skills（每个技能一个子目录，含 SKILL.md）。
    /// 与 memory 平级 —— 技能是「怎么做」的能力知识，不属于记忆生命周期
    pub fn skills_dir(&self) -> PathBuf {
        self.data_root().join("skills")
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

    /// OCR 模型目录：配置留空时取 data_root/models/ocr。
    /// 与 whisper 平级 —— 都是「按需加载的本地模型资产」，不参与缓存清理
    pub fn ocr_model_dir(&self) -> PathBuf {
        let dir = self.vision.ocr.model_dir.trim();
        if dir.is_empty() {
            self.data_root().join("models/ocr")
        } else {
            PathBuf::from(dir)
        }
    }

    /// Agent 工作区默认根目录：data_root/workspace。
    /// 文件工具的沙箱边界 —— 与 memory/session/logs 平级，
    /// 刻意不放进它们内部：模型在工作区里的读写不该碰到会话与日志数据
    pub fn workspace_path(&self) -> PathBuf {
        self.data_root().join("workspace")
    }

    /// 生效的工作区根目录列表。
    /// 配置里没写这一项 = 取默认；显式写成空数组 = 禁止全部文件操作
    pub fn workspace_roots(&self) -> Vec<PathBuf> {
        match &self.tools.workspace_roots {
            Some(list) => list.iter().map(PathBuf::from).collect(),
            None => vec![self.workspace_path()],
        }
    }

    // —— logs 相关（日志/统计数据，独立于 memory/session 生命周期）——

    /// 日志数据根：data_root/logs（存放各类日志与统计，usage 是其中一类）
    pub fn log_path(&self) -> PathBuf {
        self.data_root().join("logs")
    }

    /// 用量统计文件：data_root/logs/usage/usage.json
    pub fn usage_path(&self) -> PathBuf {
        self.log_path().join("usage/usage.json")
    }

    /// 定时任务存储文件：data_root/logs/scheduler/tasks.json
    pub fn scheduler_path(&self) -> PathBuf {
        self.log_path().join("scheduler/tasks.json")
    }

    /// 运行日志目录：data_root/logs/runtime。
    /// 与 scheduler/usage 平级而不是直接放 logs 根下 —— 轮转会产生多个文件，
    /// 散在根目录会跟那两个数据文件夹混在一起
    pub fn runtime_log_dir(&self) -> PathBuf {
        self.log_path().join("runtime")
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
    //
    // 布局见 doc/v0.7/02-file-storage.md：
    //   file/YYYY-MM-DD/           普通文件
    //   file/image/YYYY-MM-DD/     图片
    //   file/temp/                 中间态，随时可删
    //   file/cache/                可重建产物（缩略图/OCR/VLM）
    //   file/quarantine/           非法文件隔离区

    pub fn file_path(&self) -> PathBuf {
        self.file_root()
    }

    /// 图片根：固定为 file_root/image，不单独配置 ——
    /// 「图片在 file 下」这条约束不能被配置项打破
    pub fn image_root(&self) -> PathBuf {
        self.file_root().join("image")
    }

    /// 旧版默认文件根（v0.6 及以前）：project_root/file。
    /// 只用于启动时提示迁移，不再参与任何写入
    pub fn legacy_file_root(&self) -> PathBuf {
        default_project_root().join("file")
    }

    pub fn temp_file_path(&self) -> PathBuf {
        self.file_root().join("temp")
    }

    /// 可重建产物的缓存根
    pub fn file_cache_path(&self) -> PathBuf {
        self.file_root().join("cache")
    }

    /// 隔离区根
    pub fn file_quarantine_path(&self) -> PathBuf {
        self.file_root().join("quarantine")
    }

    /// 普通文件的日期归档目录：file/YYYY-MM-DD
    pub fn file_date_dir(&self, date: NaiveDate) -> PathBuf {
        self.file_root().join(format_date(date))
    }

    /// 图片的日期归档目录：file/image/YYYY-MM-DD
    pub fn image_date_dir(&self, date: NaiveDate) -> PathBuf {
        self.image_root().join(format_date(date))
    }
}

/// 日期目录统一用本地日期格式，避免 UTC 跨日把晚上的文件落到第二天
fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}
