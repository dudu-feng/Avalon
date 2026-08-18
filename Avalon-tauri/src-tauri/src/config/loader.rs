// 配置文件的加载 / 保存 / 校验 / 默认模板
//
// 只与磁盘 IO 和 toml 序列化打交道，不持有运行时状态。

use std::fs;
use std::path::PathBuf;

use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::paths::locate_config;
use super::types::*;

/// 默认 Avalon-config.toml 模板（首次启动时生成，也是 default_config 的唯一来源）
pub const DEFAULT_TEMPLATE: &str = r#"# ============================================================
#  Avalon 应用配置
# ============================================================

# 当前活跃模型（指向下方 [[models]] 中的 name）
active_model = "deepseek"

# ============================================================
#  路径配置（配置驱动：留空 = 按约定自动推导）
# ============================================================
[paths]
# 共享数据根目录（会话记忆/向量库/模型）。留空 = 项目根下的 data/
# 提示：Windows 绝对路径请用正斜杠（f:/Avalon/data），反斜杠在 TOML 中是转义符
data_root = ""
# 文件存放目录（临时文件等）。留空 = 项目根下的 file/
file_root = ""

# ============================================================
#  LLM 模型列表（连接/鉴权/模型名逐模型独立）
# ============================================================
[[models]]
name = "deepseek"                         # 唯一标识（切换锚点）
url = "https://api.deepseek.com"          # OpenAI 兼容 API 基础 URL
key = ""                                  # API Key（支持环境变量 AVALON_LLM_API_KEY 覆盖）
modelname = "deepseek-v4-flash"           # 实际模型名

# ============================================================
#  LLM 全局行为参数（所有模型共享）
# ============================================================
[llm]
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
load_mode = "lazy"                        # lazy | eager（懒加载 | 常驻热加载）
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

# ============================================================
#  向量数据库配置
# ============================================================
[vector]
backend = "memory"                        # memory（自研轻量）| sqlite（预留扩展）
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

    migrate_legacy_llm(&content, &mut config)?;

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

    // 1. models 非空
    if config.models.is_empty() {
        warnings.push("未配置模型：请在 models 列表中添加至少一个模型".to_string());
        return warnings;
    }

    // 2. active_model 存在
    if !config.models.iter().any(|m| m.name == config.active_model) {
        warnings.push(format!(
            "active_model '{}' 不在模型列表中，将回退到第一个模型",
            config.active_model
        ));
    }

    // 3. 每个模型字段非空 + name 唯一
    let mut seen = HashSet::new();
    for m in &config.models {
        if m.name.is_empty() {
            warnings.push("存在 name 为空的模型条目".to_string());
        }
        if !seen.insert(m.name.as_str()) {
            warnings.push(format!("模型 name '{}' 重复", m.name));
        }
        if m.url.is_empty() {
            warnings.push(format!("模型 '{}' 的 url 为空", m.name));
        }
        if m.key.is_empty() {
            warnings.push(format!("模型 '{}' 的 key 为空", m.name));
        }
        if m.modelname.is_empty() {
            warnings.push(format!("模型 '{}' 的 modelname 为空", m.name));
        }
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

/// 迁移旧版单模型 [llm]（含 api_key/model/base_url）→ models 列表。
/// serde 对 toml 未知字段默认忽略，故旧段里的连接字段不会让新 LlmParams 报错；
/// 这里在 models 为空时二次解析旧段，迁移为单元素列表（name=default）。
pub(crate) fn migrate_legacy_llm(content: &str, config: &mut AppConfig) -> Result<()> {
    if !config.models.is_empty() {
        return Ok(());
    }

    #[derive(Deserialize)]
    struct LegacyLlm {
        #[serde(default)] api_key: String,
        #[serde(default)] model: String,
        #[serde(default)] base_url: String,
    }
    #[derive(Deserialize)]
    struct LegacyConfig {
        #[serde(default)] llm: Option<LegacyLlm>,
    }

    let Ok(legacy) = toml::from_str::<LegacyConfig>(content) else {
        return Ok(());
    };
    if let Some(l) = legacy.llm {
        if !(l.api_key.is_empty() && l.model.is_empty() && l.base_url.is_empty()) {
            config.models.push(ModelConfig {
                name: "default".to_string(),
                url: l.base_url,
                key: l.api_key,
                modelname: l.model,
            });
            config.active_model = "default".to_string();
            println!("[Config] 检测到旧版单模型配置，已迁移为模型列表（name=default）");
        }
    }
    Ok(())
}

/// 环境变量覆盖敏感项（不落盘明文，支持按环境注入）
fn apply_env_overrides(config: &mut AppConfig) {
    if let Ok(v) = std::env::var("AVALON_LLM_API_KEY") {
        if !v.is_empty() {
            // 覆盖活跃模型的 key；无活跃则覆盖第一个
            if let Some(model) = config.models.iter_mut().find(|m| m.name == config.active_model) {
                model.key = v;
            } else if let Some(first) = config.models.first_mut() {
                first.key = v;
            }
        }
    }
}
