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
# 文件存放目录（普通文件/临时文件）。留空 = data_root/file
# 普通文件按 file_root/YYYY-MM-DD/ 归档，图片按 file_root/image/YYYY-MM-DD/ 归档
file_root = ""

# ============================================================
#  LLM 模型列表（连接/鉴权/模型名逐模型独立）
# ============================================================
[[models]]
name = "deepseek"                         # 唯一标识（切换锚点）
url = "https://api.deepseek.com"          # OpenAI 兼容 API 基础 URL
key = ""                                  # API Key（支持环境变量 AVALON_LLM_API_KEY 覆盖）
modelname = "deepseek-v4-flash"           # 实际模型名
supports_vision = false                   # 是否支持图片输入（多模态）

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

# ============================================================
#  飞书渠道（仅支持企业自建应用，商店应用无法使用长连接）
# ============================================================
[feishu]
enabled = false                           # 开关，关闭时启动不建立任何连接
app_id = ""                               # cli_ 开头
app_secret = ""                           # 敏感（支持环境变量 AVALON_FEISHU_APP_SECRET 覆盖）
domain = "https://open.feishu.cn"         # Lark 国际版填 https://open.larksuite.com
group_require_mention = true              # 群聊需 @ 机器人才响应
allow_users = []                          # open_id 白名单，留空 = 不限制
owner_open_id = ""                        # 主人 open_id，feishu_notify_owner 的收件人；留空则由第一个私聊的用户自动填充
# 会话隔离粒度：
#   isolated = 每个聊天独立上下文（私聊按人、群聊按群）
#   unified  = 所有飞书消息汇入同一个永恒会话，跨群跨私聊记忆连贯
session_mode = "isolated"
# 进度表情：在用户那条消息上标注处理进度，同一时刻只挂一个。
# 值必须是飞书的 emoji_type 枚举（OnIt / DONE / ERROR / Typing / THINKING …），
# 不能填 Unicode 字符，否则接口报 231001。任意一项留空 = 关闭该状态的标记。
queued_reaction = "OneSecond"             # 排队等待中
processing_reaction = "OnIt"              # 正在处理
done_reaction = "DONE"                    # 处理成功
failed_reaction = "ERROR"                 # 处理失败
rejected_reaction = "Sigh"                # 积压超限，本条不处理

# ============================================================
#  联网搜索（AnySearch）
#  开启后模型可主动搜索网页、读取网页正文。注意查询词会发往第三方服务。
# ============================================================
[search]
enabled = false                           # 开关，关闭时不向模型暴露搜索工具
api_key = ""                              # 敏感（支持环境变量 ANYSEARCH_API_KEY 覆盖）；留空 = 匿名，速率受限
base_url = "https://api.anysearch.com"
max_results = 5                           # 默认返回条数，接口上限 10
zone = ""                                 # cn / intl，留空由服务端判断
timeout_secs = 30
extract_limit = 8000                      # 网页正文截断字符数（接口最多返回 5 万，全塞进上下文会挤爆预算）

# ============================================================
#  Agent 基础工具沙箱（文件 / 终端）
#  限制对所有来源统一生效 —— 桌面端与飞书渠道同一套边界。
# ============================================================
[tools]
# 文件工具（read_file / write_file / delete_file / get_directory_contents）
# 允许访问的根目录，读写同边界。三种写法含义不同：
#   整行不写   = 用默认工作区 data_root/workspace（推荐，换机器不用改）
#   写成 []    = 禁止全部文件操作
#   写成列表   = 只允许列出的目录及其子目录
# 提示：Windows 绝对路径请用正斜杠，反斜杠在 TOML 中是转义符
# workspace_roots = ["f:/我的项目", "f:/Avalon/data/workspace"]

# 终端 run_shell_command 可执行的命令白名单（裸命令名，不含路径与扩展名）。
# 留空 = 禁用终端。命令不经过 shell，参数逐个传递，
# 因此 && | > ; 等元字符没有任何特殊含义，也用不了管道与重定向。
#
# 只解析 .exe/.com，脚本包装（.bat/.cmd/.ps1）一律拒绝 —— 它们会重新拉起
# 解释器，等于绕开这里的全部限制。npm / yarn / conda 因此永远用不了。
#
# 往下面加命令前请想清楚这层性质：一旦放进解释器（python / node /
# powershell）或带插件机制的工具（git 的 -c core.pager、npm 的 run 脚本），
# 限制就从「防恶意」退化成「防误操作」—— 一行代码即可绕开工作区边界。
# 同理不建议加 findstr：它的 /f: 参数能读任意文件，而这功能 read_file 已覆盖。
# dir / type 加了也没用，它们是 cmd 内建命令，没有对应的可执行文件。
shell_allowlist = ["where", "ping", "ipconfig", "tasklist", "systeminfo", "hostname"]

# ============================================================
#  视觉输入（图片理解）
#  开启后聊天输入框出现附件入口；图片会先本地缩放/去 EXIF，
#  再发送给视觉模型。图片内容会离开本机，请确认模型服务可信。
# ============================================================
[vision]
enabled = false                           # 开关，关闭时不注册图片入口
model = ""                                # 视觉模型，指向 models[].name；留空 = 活跃模型（需 supports_vision = true）
observe_mode = "always"                   # always = 首图生成并缓存观察；lazy = 需要时再生成
max_edge = 1568                           # 送模型前的最长边
max_bytes = 4194304                       # 编码后字节上限（4 MiB）
jpeg_quality = 85
detail = "auto"                           # auto / low / high；留空则不传该字段
timeout_secs = 60
concurrency = 2                           # 并发视觉调用上限
cache_ttl_days = 30                       # VLM/OCR 缓存保留天数，0 = 不自动清理
confirm_before_send = true                # 首次向某个视觉模型发送图片前弹确认

[vision.ocr]
enabled = false                           # 本地 OCR 工具（P2）
engine = "none"                           # none | onnx（tract 为后置备选，未实现）
min_confidence = 0.75                     # 低于该值在工具输出里标注低置信度
model_dir = ""                            # 模型目录；留空 = data_root/models/ocr
"#;

/// 加载配置：定位 → 不存在则生成默认模板 → 反序列化 → 回填 config_path → 环境变量覆盖
pub fn load() -> Result<AppConfig> {
    let config_path = locate_config()?;

    if !config_path.exists() {
        write_default(&config_path)?;
        log::info!(target: "config", "已生成默认配置文件: {}", config_path.display());
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

/// 保存配置：序列化（跳过 config_path）并原子写回原文件。
///
/// 先写临时文件再 rename，与 scheduler/store.rs 的写法一致。裸 `fs::write` 的
/// truncate + write 不是原子的：设置页保存与后台线程（owner 自动填充）撞在一起时
/// 可能产出半截 TOML，下次启动解析失败就会兜底成默认配置，用户整份配置全丢。
pub fn save(config: &AppConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context("序列化配置失败")?;

    if let Some(parent) = config.config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败: {}", parent.display()))?;
    }

    let tmp = config.config_path.with_extension("toml.tmp");
    fs::write(&tmp, &content)
        .with_context(|| format!("写入配置临时文件失败: {}", tmp.display()))?;
    fs::rename(&tmp, &config.config_path)
        .with_context(|| format!("原子替换配置文件失败: {}", config.config_path.display()))?;

    Ok(())
}

/// 校验配置完整性，返回警告列表
pub fn validate(config: &AppConfig) -> Vec<String> {
    let mut warnings = Vec::new();

    // 0. 飞书渠道：只在开关打开时才校验，关闭状态下留空是正常的
    if config.feishu.enabled {
        if config.feishu.app_id.is_empty() {
            warnings.push("飞书渠道已启用但 app_id 为空".to_string());
        }
        if config.feishu.app_secret.is_empty() {
            warnings.push("飞书渠道已启用但 app_secret 为空".to_string());
        }
        if config.feishu.domain.is_empty() {
            warnings.push("飞书渠道已启用但 domain 为空".to_string());
        }
    }

    // 0.5 工具沙箱：两条都只是提醒，不阻断启动 —— 用户可能就是想临时关掉某项。
    //     显式配成空（禁用）不告警，那是明确的意图；配了但不生效才需要说
    for root in config.workspace_roots() {
        if !root.is_dir() {
            warnings.push(format!(
                "工作区根目录不存在或不是目录，文件工具无法访问它: {}",
                root.display()
            ));
        }
    }
    for cmd in &config.tools.shell_allowlist {
        if crate::tool::resolve_allowed_command(cmd).is_none() {
            warnings.push(format!(
                "终端白名单里的 '{cmd}' 找不到对应的可执行文件（脚本包装 .bat/.cmd 不被支持），调用时会失败"
            ));
        }
    }

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

    // 4. 视觉配置：只在开关打开时校验
    if config.vision.enabled {
        if config.vision.model.trim().is_empty() {
            match config.active_model_config() {
                Some(m) if !m.supports_vision => warnings.push(format!(
                    "视觉已启用但未指定 [vision].model，且活跃模型 '{}' 未标记 supports_vision",
                    m.name
                )),
                None => warnings.push("视觉已启用但找不到活跃模型".to_string()),
                _ => {}
            }
        } else if let Some(m) = config
            .models
            .iter()
            .find(|m| m.name == config.vision.model.trim())
        {
            if !m.supports_vision {
                warnings.push(format!(
                    "视觉模型 '{}' 未标记 supports_vision = true，图片调用可能失败",
                    m.name
                ));
            }
        } else {
            warnings.push(format!(
                "视觉模型 '{}' 不在模型列表中",
                config.vision.model
            ));
        }
        if config.vision.ocr.enabled {
            match config.vision.ocr.engine.trim().to_ascii_lowercase().as_str() {
                "none" | "" => warnings.push("视觉 OCR 已启用但 engine = none".to_string()),
                "onnx" => {
                    // 模型是外部资产，缺失时工具不会注册；启动时就提示，免得调用时才报错。
                    // 字典不强制：RapidOCR 导出的 rec 模型自带字符表
                    let dir = config.ocr_model_dir();
                    for file in ["det.onnx", "rec.onnx"] {
                        if !dir.join(file).is_file() {
                            warnings.push(format!(
                                "视觉 OCR 缺少模型文件: {}",
                                dir.join(file).display()
                            ));
                        }
                    }
                }
                "tract" => warnings.push("OCR engine=tract 尚未实现".to_string()),
                other => warnings.push(format!("未知的 OCR 引擎: {other}")),
            }
        }
    } else if let Some(m) = config.active_model_config() {
        // 模型能力（supports_vision）和视觉总开关（[vision].enabled）分处设置页两栏，
        // 只开了前者时聊天页不会出现图片入口。这里明确提醒，免得以为是前端坏了
        if m.supports_vision {
            warnings.push(format!(
                "活跃模型 '{}' 已标记 supports_vision，但 [vision].enabled = false，聊天页不会显示图片入口",
                m.name
            ));
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
                supports_vision: false,
            });
            config.active_model = "default".to_string();
            log::info!(target: "config", "检测到旧版单模型配置，已迁移为模型列表（name=default）");
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

    if let Ok(v) = std::env::var("AVALON_FEISHU_APP_SECRET") {
        if !v.is_empty() {
            config.feishu.app_secret = v;
        }
    }

    // 用 AnySearch 官方约定的变量名，方便和已有的 CLI / MCP 配置共用一个 key
    if let Ok(v) = std::env::var("ANYSEARCH_API_KEY") {
        if !v.is_empty() {
            config.search.api_key = v;
        }
    }
}
