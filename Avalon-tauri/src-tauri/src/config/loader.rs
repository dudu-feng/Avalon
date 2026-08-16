// 配置文件的加载 / 保存 / 校验 / 默认模板
//
// 只与磁盘 IO 和 toml 序列化打交道，不持有运行时状态。

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::paths::locate_config;
use super::types::*;

/// 默认 Avalon-config.toml 模板（首次启动时生成，也是 default_config 的唯一来源）
pub const DEFAULT_TEMPLATE: &str = r#"# ============================================================
#  Avalon 应用配置
# ============================================================

# ============================================================
#  路径配置（配置驱动：留空 = 按约定自动推导）
# ============================================================
[paths]
# 共享数据根目录（会话记忆/向量库/模型）。留空 = 项目根下的 data/
data_root = ""
# 文件存放目录（临时文件等）。留空 = 项目根下的 file/
file_root = ""

# ============================================================
#  LLM 模型配置
# ============================================================
[llm]
api_key = ""                              # 支持环境变量 AVALON_LLM_API_KEY 覆盖
model = "deepseek-v4-flash"
base_url = "https://api.deepseek.com"
chat_temperature = 0.7                    # 对话层温度
json_temperature = 0.1                    # JSON/动作层温度
timeout_secs = 120                        # 请求超时秒数

# ============================================================
#  Embedding 向量化配置
# ============================================================
[embedding]
mode = "local"                            # local | api
local_model = "bge-small-zh-v1.5"
device = "cpu"
api_key = ""
api_model = "text-embedding-3-small"
api_base_url = ""

# ============================================================
#  会话记忆配置
# ============================================================
[session_memory]
compress_threshold = 10000                # 输入 token 超此阈值触发自动压缩
max_chunks = 10                           # 压缩块超此数量触发渐进式总结
context_chunks = 5                        # 系统提示加载最近 N 个压缩块
search_mode = "hybrid"                    # semantic | keyword | hybrid

# ============================================================
#  Whisper 语音转写配置
# ============================================================
[whisper]
model_name = "medium"
device = "cpu"
"#;

/// 加载配置：定位 → 不存在则生成默认模板 → 反序列化 → 回填 config_path → 环境变量覆盖
pub fn load() -> Result<AppConfig> {
    let config_path = locate_config()?;

    if !config_path.exists() {
        write_default(&config_path)?;
        println!("[Config] 已生成默认配置文件: {}", config_path.display());
    }

    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("读取配置文件失败: {}", config_path.display()))?;

    let mut config: AppConfig = toml::from_str(&content)
        .with_context(|| format!("解析配置文件失败: {}", config_path.display()))?;

    config.config_path = config_path;

    apply_env_overrides(&mut config);

    Ok(config)
}

/// 保存配置：序列化（跳过 config_path）并写回原文件
pub fn save(config: &AppConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context("序列化配置失败")?;

    if let Some(parent) = config.config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }

    fs::write(&config.config_path, content)
        .with_context(|| format!("写入配置文件失败: {}", config.config_path.display()))?;

    Ok(())
}

/// 校验配置完整性，返回警告列表
pub fn validate(config: &AppConfig) -> Vec<String> {
    let mut warnings = Vec::new();

    if config.llm.api_key.is_empty() {
        warnings.push("LLM API Key 为空，请在配置中设置 llm.api_key".to_string());
    }
    if config.llm.model.is_empty() {
        warnings.push("LLM 模型名称为空 (llm.model)".to_string());
    }
    if config.llm.base_url.is_empty() {
        warnings.push("LLM API base_url 为空 (llm.base_url)".to_string());
    }

    warnings
}

/// 内存默认配置（加载失败时的兜底，不写盘）。
/// 默认值单一来源：直接解析 DEFAULT_TEMPLATE。
pub fn default_config() -> AppConfig {
    let config_path = locate_config().unwrap_or_else(|_| PathBuf::from("Avalon-config.toml"));
    let mut config: AppConfig =
        toml::from_str(DEFAULT_TEMPLATE).expect("默认配置模板解析失败（应保证模板合法）");
    config.config_path = config_path;
    config
}

/// 首次启动：写入默认模板到指定位置
fn write_default(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }
    fs::write(path, DEFAULT_TEMPLATE)
        .with_context(|| format!("写入默认配置失败: {}", path.display()))?;
    Ok(())
}

/// 环境变量覆盖敏感项（不落盘明文，支持按环境注入）
fn apply_env_overrides(config: &mut AppConfig) {
    if let Ok(v) = std::env::var("AVALON_LLM_API_KEY") {
        if !v.is_empty() {
            config.llm.api_key = v;
        }
    }
}
