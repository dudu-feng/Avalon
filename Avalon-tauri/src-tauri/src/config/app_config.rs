// 配置中心 —— 直接加载 Python 项目的 .env 文件
//
// 与 Python 版 env_config.py 功能对应：
//   - 解析 .env 格式（KEY=VALUE、#注释、空行）
//   - 保存时保持原文件的注释和行顺序
//   - 解析 "64k"/"20000" 这种阈值格式
//
// 默认加载路径：{PROJECT_ROOT}/Avalon-python/agent/.env
// 也可通过环境变量 AVALON_ENV_PATH 自定义

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================
//  顶层配置结构
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// LLM 模型配置
    pub llm: LlmConfig,

    /// 文件路径配置
    pub paths: PathsConfig,

    /// 向量数据库配置
    pub vector_db: VectorDbConfig,

    /// Embedding 模型配置
    pub embedding: EmbeddingConfig,

    /// 会话记忆配置
    pub session_memory: SessionMemoryConfig,

    /// .env 文件的实际路径（保存时写回原位置）
    #[serde(skip)]
    pub env_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub prompt_file_path: String,
    pub memory_path: String,
    pub session_path: String,
    pub session_index_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDbConfig {
    pub vector_db_path: String,
    pub model_cache_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// "local" 使用本地模型，"api" 使用 API 模型
    pub mode: String,
    pub local_model: String,
    pub device: String,
    pub api_key: String,
    pub api_model: String,
    pub api_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryConfig {
    /// 自动压缩 token 阈值
    pub compress_threshold: usize,
    /// 渐进式总结触发上限
    pub max_chunks: usize,
    /// 系统提示中加载的最近压缩块数量
    pub context_chunks: usize,
    /// 预留的搜索模式
    pub search_mode: String,
}

// ============================================================
//  .env 文件解析器（保留注释、空行、行顺序）
// ============================================================

/// .env 文件的一行
enum EnvLine {
    /// 空行
    Blank,
    /// 注释行（保留原内容）
    Comment(String),
    /// 键值对行：序号+原始行文本的可写回引用（保存时写回）
    Kv {
        key: String,
        value: String,
    },
}

/// 解析后的 .env 文件内容（保留结构）
struct EnvFile {
    lines: Vec<EnvLine>,
}

impl EnvFile {
    /// 从 .env 文本内容解析
    fn parse(content: &str) -> Self {
        let mut lines = Vec::new();
        for raw in content.lines() {
            let trimmed = raw.trim();

            if trimmed.is_empty() {
                lines.push(EnvLine::Blank);
                continue;
            }
            if trimmed.starts_with('#') {
                lines.push(EnvLine::Comment(raw.to_string()));
                continue;
            }

            // KEY=VALUE 格式
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                lines.push(EnvLine::Kv { key, value });
            } else {
                // 无法解析的行当作注释保留
                lines.push(EnvLine::Comment(raw.to_string()));
            }
        }
        EnvFile { lines }
    }

    /// 取所有键值对（生成配置用）
    fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for line in &self.lines {
            if let EnvLine::Kv { key, value } = line {
                map.insert(key.clone(), value.clone());
            }
        }
        map
    }

    /// 批量更新指定键的值（不存在则忽略，保存时保持原顺序）
    fn update(&mut self, updates: &HashMap<String, String>) {
        for line in &mut self.lines {
            if let EnvLine::Kv { key, value } = line {
                if let Some(new_val) = updates.get(key) {
                    *value = new_val.clone();
                }
            }
        }
    }

    /// 追加一个键值对（保存时写入末尾，用于缺失字段）
    fn append_kv(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.lines.push(EnvLine::Kv {
            key: key.into(),
            value: value.into(),
        });
    }

    /// 序列化回 .env 文本
    fn to_string(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                EnvLine::Blank => out.push('\n'),
                EnvLine::Comment(c) => {
                    out.push_str(c);
                    out.push('\n');
                }
                EnvLine::Kv { key, value } => {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(value);
                    out.push('\n');
                }
            }
        }
        out
    }
}

// ============================================================
//  派生路径（便捷方法，后续向量模块使用）
// ============================================================

#[allow(dead_code)]
impl AppConfig {
    /// 本地 embedding 模型完整路径 = model_cache_dir + local_model
    pub fn local_embedding_model_path(&self) -> PathBuf {
        PathBuf::from(&self.vector_db.model_cache_dir).join(&self.embedding.local_model)
    }

    /// ZVec 持久化路径 = vector_db_path + "zvec"
    pub fn zvec_db_path(&self) -> PathBuf {
        PathBuf::from(&self.vector_db.vector_db_path).join("zvec")
    }
}

// ============================================================
//  配置文件管理
// ============================================================

impl AppConfig {
    /// 决定 .env 文件路径：
    ///   1. 环境变量 AVALON_ENV_PATH
    ///   2. 默认：{项目根}/Avalon-python/agent/.env
    pub fn env_file_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("AVALON_ENV_PATH") {
            return Ok(PathBuf::from(p));
        }

        // 推导项目根：src-tauri → Avalon-tauri → Avalon
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok();
        let base: PathBuf = if let Some(m) = manifest_dir {
            // m = Avalon/Avalon-tauri/src-tauri
            PathBuf::from(m)
                .parent()      // Avalon-tauri
                .and_then(|p| p.parent()) // Avalon
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("..").join(".."))
        } else {
            // 运行时推导：当前 exe 所在目录
            std::env::current_exe()
                .ok()
                .and_then(|e| {
                    e.parent()?
                        .parent()?
                        .parent()?
                        .parent()
                        .map(|p| p.to_path_buf())
                })
                .unwrap_or_else(|| PathBuf::from("."))
        };

        Ok(base.join("Avalon-python").join("agent").join(".env"))
    }

    /// 加载配置（.env 文件）。若文件不存在则生成默认 .env 写入对应位置。
    pub fn load() -> Result<Self> {
        let path = Self::env_file_path()?;

        let (env_map, env_file) = if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("读取 .env 文件失败: {}", path.display()))?;
            let env_file = EnvFile::parse(&content);
            let env_map = env_file.to_map();
            (env_map, Some(env_file))
        } else {
            println!("[Config] 未找到 .env 文件，将生成默认配置到: {}", path.display());
            (HashMap::new(), None)
        };

        // 键名对齐 Python .env
        let get = |k: &str, d: &str| -> String {
            env_map.get(k).cloned().unwrap_or_else(|| d.to_string())
        };
        let get_int = |k: &str, d: usize| -> usize {
            env_map
                .get(k)
                .and_then(|v| parse_size_with_k(v).ok())
                .unwrap_or(d)
        };

        let mut config = AppConfig {
            llm: LlmConfig {
                api_key: get("default_api_key", ""),
                model: get("default_model", "mimo-v2.5"),
                base_url: get(
                    "default_model_base_url",
                    "https://token-plan-cn.xiaomimimo.com/v1",
                ),
            },
            paths: PathsConfig {
                prompt_file_path: get("prompt_file_path", ""),
                memory_path: get("memory_path", ""),
                session_path: get("session_path", ""),
                session_index_path: get("session_index_path", ""),
            },
            vector_db: VectorDbConfig {
                vector_db_path: get("vector_db_path", ""),
                model_cache_dir: get("model_cache_dir", ""),
            },
            embedding: EmbeddingConfig {
                mode: get("embedding_mode", "local"),
                local_model: get("local_embedding_model", "bge-small-zh-v1.5"),
                device: get("embedding_device", "cpu"),
                api_key: get("api_embedding_key", ""),
                api_model: get("api_embedding_model", ""),
                api_base_url: get("api_embedding_base_url", ""),
            },
            session_memory: SessionMemoryConfig {
                compress_threshold: get_int("session_memory_compress_threshold", 20000),
                max_chunks: get_int("session_memory_max_chunks", 10),
                context_chunks: get_int("session_memory_context_chunks", 5),
                search_mode: get("session_memory_search_mode", "hybrid"),
            },
            env_path: path.clone(),
        };

        // 若 .env 不存在：生成默认 .env 到正确位置
        if env_file.is_none() {
            // 填充默认路径（对齐 Python 版的 f:\Avalon\data\... 风格，优先写绝对路径）
            let data_dir = derive_default_data_dir();
            config.paths.prompt_file_path = sub(&data_dir, "memory\\prompt");
            config.paths.memory_path = sub(&data_dir, "memory");
            config.paths.session_path = sub(&data_dir, "memory\\session");
            config.paths.session_index_path = sub(&data_dir, "memory\\session\\index.json");
            config.vector_db.vector_db_path = sub(&data_dir, "vector\\vector_db");
            config.vector_db.model_cache_dir = sub(&data_dir, "vector\\models\\embedding");

            config.save_to_env_file()?;
            println!("[Config] 已创建默认 .env 文件: {}", path.display());
        }

        Ok(config)
    }

    /// 保存配置：把 AppConfig 字段写回对应的 .env KEY 行
    pub fn save(&self) -> Result<()> {
        self.save_to_env_file()
    }

    fn save_to_env_file(&self) -> Result<()> {
        let path = &self.env_path;

        // 读取现有内容（保留注释、顺序）
        let mut env_file = if path.exists() {
            let content = fs::read_to_string(path)
                .with_context(|| format!("读取 .env 文件失败: {}", path.display()))?;
            EnvFile::parse(&content)
        } else {
            EnvFile::parse(DEFAULT_ENV_TEMPLATE)
        };

        // 构建键值对更新表（按 Python .env 的 key 名）
        let mut updates: HashMap<String, String> = HashMap::new();
        updates.insert("default_api_key".into(), self.llm.api_key.clone());
        updates.insert("default_model".into(), self.llm.model.clone());
        updates.insert("default_model_base_url".into(), self.llm.base_url.clone());

        updates.insert("prompt_file_path".into(), self.paths.prompt_file_path.clone());
        updates.insert("memory_path".into(), self.paths.memory_path.clone());
        updates.insert("session_path".into(), self.paths.session_path.clone());
        updates.insert("session_index_path".into(), self.paths.session_index_path.clone());

        updates.insert("vector_db_path".into(), self.vector_db.vector_db_path.clone());
        updates.insert("model_cache_dir".into(), self.vector_db.model_cache_dir.clone());

        updates.insert("embedding_mode".into(), self.embedding.mode.clone());
        updates.insert("local_embedding_model".into(), self.embedding.local_model.clone());
        updates.insert("embedding_device".into(), self.embedding.device.clone());
        updates.insert("api_embedding_key".into(), self.embedding.api_key.clone());
        updates.insert("api_embedding_model".into(), self.embedding.api_model.clone());
        updates.insert("api_embedding_base_url".into(), self.embedding.api_base_url.clone());

        updates.insert("session_memory_search_mode".into(), self.session_memory.search_mode.clone());
        updates.insert(
            "session_memory_compress_threshold".into(),
            format_size_with_k(self.session_memory.compress_threshold),
        );
        updates.insert(
            "session_memory_max_chunks".into(),
            self.session_memory.max_chunks.to_string(),
        );
        updates.insert(
            "session_memory_context_chunks".into(),
            self.session_memory.context_chunks.to_string(),
        );

        env_file.update(&updates);

        // 确保缺失的 KEY 追加在末尾（兼容老版本 .env 缺字段）
        for (k, v) in &updates {
            let exists = env_file.lines.iter().any(|line| {
                if let EnvLine::Kv { key, .. } = line {
                    key == k
                } else {
                    false
                }
            });
            if !exists {
                env_file.append_kv(k, v);
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建 .env 目录失败: {}", parent.display()))?;
        }

        fs::write(path, env_file.to_string())
            .with_context(|| format!("写入 .env 文件失败: {}", path.display()))?;

        Ok(())
    }

    /// 校验配置完整性（返回警告信息列表）
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.llm.api_key.is_empty() {
            warnings.push("LLM API Key 为空，请在配置中设置 default_api_key".to_string());
        }
        if self.llm.model.is_empty() {
            warnings.push("LLM 模型名称为空 (default_model)".to_string());
        }
        if self.llm.base_url.is_empty() {
            warnings.push("LLM API base_url 为空 (default_model_base_url)".to_string());
        }

        warnings
    }

    /// 生成一份默认配置（.env 加载失败时的降级兜底，不做文件写入）
    pub fn default_config() -> Self {
        let env_path = Self::env_file_path().unwrap_or_else(|_| PathBuf::from(".env"));
        let data_dir = derive_default_data_dir();
        AppConfig {
            llm: LlmConfig {
                api_key: String::new(),
                model: "mimo-v2.5".to_string(),
                base_url: "https://token-plan-cn.xiaomimimo.com/v1".to_string(),
            },
            paths: PathsConfig {
                prompt_file_path: sub(&data_dir, "memory\\prompt"),
                memory_path: sub(&data_dir, "memory"),
                session_path: sub(&data_dir, "memory\\session"),
                session_index_path: sub(&data_dir, "memory\\session\\index.json"),
            },
            vector_db: VectorDbConfig {
                vector_db_path: sub(&data_dir, "vector\\vector_db"),
                model_cache_dir: sub(&data_dir, "vector\\models\\embedding"),
            },
            embedding: EmbeddingConfig {
                mode: "local".to_string(),
                local_model: "bge-small-zh-v1.5".to_string(),
                device: "cpu".to_string(),
                api_key: String::new(),
                api_model: String::new(),
                api_base_url: String::new(),
            },
            session_memory: SessionMemoryConfig {
                compress_threshold: 64000,
                max_chunks: 10,
                context_chunks: 5,
                search_mode: "hybrid".to_string(),
            },
            env_path,
        }
    }
}

// ============================================================
//  辅助函数
// ============================================================

/// 解析带 "k" 后缀的数字："64k" → 64000，"20000" → 20000
fn parse_size_with_k(raw: &str) -> Result<usize> {
    let s = raw.trim().to_lowercase();
    let n = if let Some(num) = s.strip_suffix('k') {
        let f: f64 = num.parse().with_context(|| format!("无效数字: {}", num))?;
        (f * 1000.0) as usize
    } else {
        s.parse().with_context(|| format!("无效数字: {}", s))?
    };
    Ok(n)
}

/// 整数序列化回 .env：优先用 "64k" 这种人类可读格式
fn format_size_with_k(n: usize) -> String {
    if n >= 1000 && n % 1000 == 0 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

/// {base}\\{suffix} 的字符串路径
fn sub(base: &Path, suffix: &str) -> String {
    base.join(suffix).to_string_lossy().to_string()
}

/// 生成默认数据目录，对齐 Python 版 f:\\Avalon\\data 风格
fn derive_default_data_dir() -> PathBuf {
    // 优先写 f:\Avalon\data（与 Python 版保持一致）；推导方式：
    //   .env 在 {root}/Avalon-python/agent/.env → root = {root}
    let env = std::env::var("AVALON_ENV_PATH").ok();
    let root: PathBuf = if let Some(env_path) = env {
        PathBuf::from(&env_path)
            .parent()       // agent
            .and_then(|p| p.parent()) // Avalon-python
            .and_then(|p| p.parent()) // {root}
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        // CARGO_MANIFEST_DIR = Avalon/Avalon-tauri/src-tauri
        std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .ok()
            .and_then(|m| {
                m.parent()?.parent().map(|p| p.to_path_buf())
            })
            .unwrap_or_else(|| PathBuf::from("..").join(".."))
    };

    root.join("data")
}

/// 默认 .env 模板（首次启动时写入）
const DEFAULT_ENV_TEMPLATE: &str = r#"# ============================================================
#  LLM 模型配置
# ============================================================
default_api_key=
default_model=mimo-v2.5
default_model_base_url=https://token-plan-cn.xiaomimimo.com/v1

# ============================================================
#  路径配置（绝对路径）
# ============================================================
prompt_file_path=
memory_path=
session_index_path=
session_path=

# ============================================================
#  向量数据库配置
# ============================================================
vector_db_path=
model_cache_dir=

# ============================================================
#  Embedding 模型配置
# ============================================================
embedding_mode=local
local_embedding_model=bge-small-zh-v1.5
embedding_device=cpu
api_embedding_key=sk-xxxx
api_embedding_model=text-embedding-3-small
api_embedding_base_url=

# ============================================================
#  会话记忆配置
# ============================================================
session_memory_search_mode=hybrid
session_memory_compress_threshold=64k
session_memory_max_chunks=10
session_memory_context_chunks=5
"#;
