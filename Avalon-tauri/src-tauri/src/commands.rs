// Tauri IPC 命令定义
//
// 前端通过 invoke("command_name", { params }) 调用这些命令。
// 分两类：配置管理命令 + LLM 调用命令（LLM 命令由 engine 层后续编排）。

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::ipc::Channel;
use tauri::State;

use crate::channel::contact::{Contact, ContactStore, OwnerContact};
use crate::channel::{ChannelKind, ChannelManager, ChannelStatus};
use crate::config::{AppConfig, ConfigStore};
use crate::engine::{Engine, EngineEvent};
use crate::file::{
    CleanupReport, FileSource, FileStore, SaveRequest, StoredFile, MAX_FILE_BYTES,
    QUARANTINE_TTL_DAYS, TEMP_TTL_HOURS,
};
use crate::llm::{CompressResult, LlmState};
use crate::prompt::{build_compress_prompt, PromptAssembler};
use crate::scheduler::{parse_schedule, ScheduledTask, TaskSource, TaskStore};
use crate::session::{ContextUsage, LoadHistoryResult, SessionData, SessionMeta};
use crate::skill::{Skill, SkillDetail, SkillRegistry};
use crate::soul::{EntryCategory, SoulEntry, SoulEntryDetail, SoulRegistry};
use crate::usage::{DailyUsageRow, UsageStore};
use crate::vector::{RebuildProgress, RebuildStats};
use crate::vision::{ChatAttachment, Observation, VisionCapability, VisionService};

// ============ 配置管理 ============

/// 获取当前配置快照
#[tauri::command]
pub fn get_config(store: State<'_, ConfigStore>) -> Result<AppConfig, String> {
    Ok(store.get())
}

/// 切换活跃模型（校验存在后写回配置），返回校验警告列表
#[tauri::command]
pub fn set_active_model(
    name: String,
    store: State<'_, ConfigStore>,
) -> Result<Vec<String>, String> {
    store.set_active_model(&name).map_err(|e| e.to_string())
}

/// 保存配置（写回 Avalon-config.toml），返回校验警告列表
#[tauri::command]
pub fn save_config(
    mut new_config: AppConfig,
    store: State<'_, ConfigStore>,
    vision: State<'_, Arc<VisionService>>,
) -> Result<Vec<String>, String> {
    // 前端不会传 config_path（serde(skip)），继承当前值
    let current = store.get();
    new_config.config_path = current.config_path;

    // 关闭 OCR、换引擎或换模型目录后，旧会话不再复用；释放已加载的 ONNX 会话
    let ocr_changed = current.vision.ocr.enabled != new_config.vision.ocr.enabled
        || current.vision.ocr.engine != new_config.vision.ocr.engine
        || current.vision.ocr.model_dir != new_config.vision.ocr.model_dir;

    let warnings = store.save(new_config).map_err(|e| e.to_string())?;
    if ocr_changed {
        vision.unload_ocr();
        log::info!(target: "vision", "OCR 配置已变更，已卸载已加载的引擎");
    }
    Ok(warnings)
}

/// 校验当前配置，返回警告列表
#[tauri::command]
pub fn validate_config(store: State<'_, ConfigStore>) -> Result<Vec<String>, String> {
    Ok(store.validate())
}

/// 获取配置文件路径
#[tauri::command]
pub fn get_config_path(store: State<'_, ConfigStore>) -> Result<String, String> {
    Ok(store.get().config_path.to_string_lossy().to_string())
}

/// 初始化应用：返回配置校验结果（前端据此判断是否引导配置）
#[tauri::command]
pub fn init_app(store: State<'_, ConfigStore>) -> Result<Vec<String>, String> {
    let warnings = store.validate();
    for w in &warnings {
        log::warn!(target: "config", "配置校验: {w}");
    }
    Ok(warnings)
}

// ============ 文件存储 ============

/// 保存聊天附件。
///
/// WebView 里的 File 没有可读的本地路径，所以只能传 base64。解码、哈希与落盘
/// 都是阻塞操作，放进 spawn_blocking，避免 50MB 附件卡住主线程。
#[tauri::command]
pub async fn save_attachment(
    file_name: String,
    mime: String,
    data_base64: String,
    source: String,
    session_id: Option<String>,
    message_id: Option<String>,
    store: State<'_, Arc<FileStore>>,
) -> Result<StoredFile, String> {
    let source = FileSource::parse(&source).ok_or_else(|| format!("未知的文件来源: {source}"))?;

    // 先按 base64 长度预检：超限直接拒绝，不必解码一份 50MB 的字节
    if data_base64.len() as u64 > MAX_FILE_BYTES.saturating_mul(4) / 3 + 16 {
        return Err(format!("文件超过上限 {} MB", MAX_FILE_BYTES / 1024 / 1024));
    }

    let bytes = STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("文件超过上限 {} MB", MAX_FILE_BYTES / 1024 / 1024));
    }

    let store = store.inner().clone();
    tokio::task::spawn_blocking(move || {
        store
            .save(SaveRequest {
                bytes: &bytes,
                original_name: &file_name,
                declared_mime: Some(&mime),
                source,
                session_id: session_id.as_deref(),
                message_id: message_id.as_deref(),
            })
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("保存任务失败: {e}"))?
}

/// 相对路径 → 绝对路径（已做越界校验），供前端预览或交给系统程序打开
#[tauri::command]
pub fn resolve_file_path(
    relative: String,
    store: State<'_, Arc<FileStore>>,
) -> Result<String, String> {
    store
        .resolve(&relative)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|e| format!("{e:#}"))
}

/// 清理过期 temp / quarantine / 视觉缓存。TTL 不传时用默认值或配置值
#[tauri::command]
pub async fn cleanup_files(
    temp_ttl_hours: Option<i64>,
    quarantine_ttl_days: Option<i64>,
    cache_ttl_days: Option<i64>,
    store: State<'_, Arc<FileStore>>,
    config: State<'_, ConfigStore>,
) -> Result<CleanupReport, String> {
    let hours = temp_ttl_hours.unwrap_or(TEMP_TTL_HOURS);
    let days = quarantine_ttl_days.unwrap_or(QUARANTINE_TTL_DAYS);
    // 缓存 TTL 以配置为默认值；命令参数只用于临时覆盖
    let cache_days = cache_ttl_days.unwrap_or(config.get().vision.cache_ttl_days);
    let store = store.inner().clone();

    tokio::task::spawn_blocking(move || {
        let now = chrono::Local::now();
        let mut report = store
            .cleanup_with_ttl(now, hours, days)
            .map_err(|e| format!("{e:#}"))?;
        let cache = store
            .cleanup_cache(now, cache_days)
            .map_err(|e| format!("{e:#}"))?;

        report.removed_files += cache.removed_files;
        report.removed_dirs += cache.removed_dirs;
        report.freed_bytes += cache.freed_bytes;
        Ok(report)
    })
    .await
    .map_err(|e| format!("清理任务失败: {e}"))?
}

// ============ 视觉 ============

/// 视觉能力状态：前端据此决定是否显示附件入口
#[tauri::command]
pub fn vision_status(vision: State<'_, Arc<VisionService>>) -> VisionCapability {
    vision.capability()
}

/// 对单张已落盘图片生成（或读取缓存的）观察文本
#[tauri::command]
pub async fn analyze_image(
    path: String,
    vision: State<'_, Arc<VisionService>>,
) -> Result<Observation, String> {
    vision.observe(&path).await.map_err(|e| format!("{e:#}"))
}

/// 返回缩略图相对路径（必要时生成一次），供需要路径的调用方使用
#[tauri::command]
pub async fn get_thumbnail(
    path: String,
    vision: State<'_, Arc<VisionService>>,
) -> Result<String, String> {
    vision.thumbnail(&path).await.map_err(|e| format!("{e:#}"))
}

/// 返回缩略图 data URL（必要时生成一次）。WebView 直接渲染，
/// 不依赖 Tauri asset 协议，因而也不受可配置 file_root 的 scope 限制
#[tauri::command]
pub async fn get_thumbnail_data_url(
    path: String,
    vision: State<'_, Arc<VisionService>>,
) -> Result<String, String> {
    vision
        .thumbnail_data_url(&path)
        .await
        .map_err(|e| format!("{e:#}"))
}

// ============ LLM 调用 ============

/// 会话压缩：非流式 JSON，返回 summary + keywords。
#[tauri::command]
pub async fn llm_compress(
    session_data: String,
    config: State<'_, ConfigStore>,
    llm: State<'_, LlmState>,
) -> Result<CompressResult, String> {
    let cfg = config.get();
    let model = cfg
        .active_model_config()
        .cloned()
        .ok_or_else(|| "未配置活跃模型（active_model 无效）".to_string())?;
    let client = llm.client(model, cfg.llm.clone());
    let (system, user) = build_compress_prompt(&session_data);
    client
        .compress(&system, &user)
        .await
        .map_err(|e| e.to_string())
}

// ============ Engine 编排 ============

/// 主聊天入口：跑完整 ReAct 双层循环，中间态经 Channel<EngineEvent> 逐事件推送。
/// 会话生命周期：调用方先 invoke("init_session")，chat 后 invoke("save_session")（决策 D3）。
#[tauri::command]
pub async fn chat(
    user_input: String,
    channel_name: String,
    attachments: Option<Vec<ChatAttachment>>,
    engine: State<'_, Arc<Engine>>,
    on_event: Channel<EngineEvent>,
) -> Result<(), String> {
    let cancel = engine.begin_chat(&channel_name);
    let input = crate::engine::UserInput {
        for_model: &user_input,
        for_history: &user_input,
        meta: None,
        attachments: attachments.unwrap_or_default(),
    };
    engine
        .run_with_input(input, &channel_name, cancel, move |ev| {
            let _ = on_event.send(ev);
        })
        .await
        .map_err(|e| e.to_string())
}

/// 停止当前 channel 正在进行的流式生成（置位取消标志，chat 提前收尾返回部分结果）
#[tauri::command]
pub fn stop_chat(channel_name: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.stop_chat(&channel_name);
    Ok(())
}

/// 初始化会话（channel 维度）：active 复用 / 否则新建
#[tauri::command]
pub fn init_session(channel_name: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine
        .init_session(&channel_name)
        .map_err(|e| e.to_string())
}

/// 新建会话：归档当前（若非空），创建新的 active 会话，返回新会话完整数据
#[tauri::command]
pub async fn create_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    engine
        .create_session(&channel_name)
        .await
        .map_err(|e| e.to_string())
}

/// 读取当前会话完整数据（供前端加载历史 / 判断状态）
#[tauri::command]
pub fn get_current_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    engine
        .get_current_session(&channel_name)
        .map_err(|e| e.to_string())
}

/// 读取当前会话上下文用量（最大输入 token vs 压缩阈值，供前端圆形进度条展示）
#[tauri::command]
pub fn get_context_usage(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<ContextUsage, String> {
    engine
        .get_context_usage(&channel_name)
        .map_err(|e| e.to_string())
}

/// 归档当前会话（压缩 + 移入 history）
#[tauri::command]
pub async fn save_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    engine
        .save_session(&channel_name)
        .await
        .map_err(|e| e.to_string())
}

/// 主动压缩当前会话上下文（不归档），返回是否实际压缩（空会话返回 false）
#[tauri::command]
pub async fn compress_session(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<bool, String> {
    engine
        .compress_session(&channel_name)
        .await
        .map_err(|e| e.to_string())
}

/// 列出全部会话元信息（active 置顶 + 归档按时间倒序），供会话历史列表
#[tauri::command]
pub fn list_sessions(
    channel_name: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<Vec<SessionMeta>, String> {
    engine
        .list_sessions(&channel_name)
        .map_err(|e| e.to_string())
}

/// 切换会话：归档当前（若非空），将目标历史会话设为 active 并返回其完整数据
#[tauri::command]
pub async fn switch_session(
    channel_name: String,
    id: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<SessionData, String> {
    engine
        .switch_session(&channel_name, &id)
        .await
        .map_err(|e| e.to_string())
}

/// 渐进式加载历史块：before_chunk=None 读最新块，否则读更早一块（供前端渐进式回溯会话历史）
#[tauri::command]
pub fn load_session_history(
    id: String,
    before_chunk: Option<u64>,
    engine: State<'_, Arc<Engine>>,
) -> Result<LoadHistoryResult, String> {
    engine
        .load_session_history(&id, before_chunk)
        .map_err(|e| e.to_string())
}

/// 删除归档会话（目录 + 向量库该会话 chunk 一并清理）
#[tauri::command]
pub fn delete_session(id: String, engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.delete_session(&id).map_err(|e| e.to_string())
}

/// 重命名会话标题（活跃或归档均可）
#[tauri::command]
pub fn rename_session(
    channel_name: String,
    id: String,
    title: String,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    engine
        .rename_session(&channel_name, &id, &title)
        .map_err(|e| e.to_string())
}

/// 重建会话向量库：清空 + 重扫 history/current + 重新入库（设置页维护操作）
/// 逐 session 处理时经 Channel<RebuildProgress> 上报进度。
/// 同步 CPU/IO 密集，用 spawn_blocking 避免阻塞主线程。
#[tauri::command]
pub async fn rebuild_memory_index(
    engine: State<'_, Arc<Engine>>,
    on_event: Channel<RebuildProgress>,
) -> Result<RebuildStats, String> {
    let engine = engine.inner().clone();
    tokio::task::spawn_blocking(move || {
        engine.rebuild_memory_index(move |p| {
            let _ = on_event.send(p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

// ============ 用量统计 ============

/// 查询最近 N 天用量（按「天 × 模型」展平），供首页控制台报表读取
#[tauri::command]
pub fn query_daily_usage(
    days: usize,
    usage: State<'_, Arc<UsageStore>>,
) -> Result<Vec<DailyUsageRow>, String> {
    Ok(usage.query_daily(days))
}

// ============ 灵魂 / 画像 ============

/// 列出可见灵魂条目（不含 system 基本设定，前端渲染条目列表）
#[tauri::command]
pub fn list_soul_entries(registry: State<'_, Arc<SoulRegistry>>) -> Result<Vec<SoulEntry>, String> {
    registry.list_visible().map_err(|e| e.to_string())
}

/// 读取单条灵魂条目 + 正文（前端模态框 / agent 查看内容）
#[tauri::command]
pub fn get_soul_entry(
    id: String,
    registry: State<'_, Arc<SoulRegistry>>,
) -> Result<SoulEntryDetail, String> {
    registry.get(&id).map_err(|e| e.to_string())
}

/// 切换条目是否注入 system prompt，写后刷新提示词缓存使下一轮对话生效
#[tauri::command]
pub fn set_soul_entry_enabled(
    id: String,
    enabled: bool,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<SoulEntry, String> {
    let entry = registry
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(entry)
}

/// 新增灵魂条目（kind=user），写后刷新提示词缓存使下一轮对话生效
#[tauri::command]
pub fn create_soul_entry(
    category: String,
    title: String,
    content: String,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<SoulEntry, String> {
    let category = EntryCategory::parse(&category)
        .ok_or_else(|| "category 应为 soul 或 profile".to_string())?;
    let entry = registry
        .create(category, &title, &content)
        .map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(entry)
}

/// 编辑灵魂条目（title/content 至少一个；system 只读），写后刷新提示词缓存
#[tauri::command]
pub fn update_soul_entry(
    id: String,
    title: Option<String>,
    content: Option<String>,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<SoulEntry, String> {
    let entry = registry
        .update(&id, title.as_deref(), content.as_deref())
        .map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(entry)
}

/// 删除灵魂条目（system 与 seed 不可删），写后刷新提示词缓存
#[tauri::command]
pub fn delete_soul_entry(
    id: String,
    registry: State<'_, Arc<SoulRegistry>>,
    prompt: State<'_, Arc<PromptAssembler>>,
) -> Result<(), String> {
    registry.delete(&id).map_err(|e| e.to_string())?;
    prompt.refresh();
    Ok(())
}

// ============ 技能 / Skills ============

/// 列出全部技能（name + description，正文经 get_skill 按需读取）
#[tauri::command]
pub fn list_skills(registry: State<'_, Arc<SkillRegistry>>) -> Result<Vec<Skill>, String> {
    registry.list().map_err(|e| e.to_string())
}

/// 读取单个技能正文（前端详情/编辑模态框，agent 的 use_skill 也走此读取）
#[tauri::command]
pub fn get_skill(
    name: String,
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<SkillDetail, String> {
    registry.get(&name).map_err(|e| e.to_string())
}

/// 新增技能（name 需合法：ascii 字母数字 + -/_，用作目录名）
#[tauri::command]
pub fn create_skill(
    name: String,
    description: String,
    content: String,
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<Skill, String> {
    registry
        .create(&name, &description, &content)
        .map_err(|e| e.to_string())
}

/// 编辑技能（name 只读，仅改 description + content）
#[tauri::command]
pub fn update_skill(
    name: String,
    description: String,
    content: String,
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<Skill, String> {
    registry
        .update(&name, &description, &content)
        .map_err(|e| e.to_string())
}

/// 删除技能（删除其整个目录）
#[tauri::command]
pub fn delete_skill(name: String, registry: State<'_, Arc<SkillRegistry>>) -> Result<(), String> {
    registry.delete(&name).map_err(|e| e.to_string())
}

// ============ 定时任务 ============

/// 创建定时任务（用户 UI 入口，source=User）。参数扁平化，返回完整任务。
#[tauri::command]
pub fn create_scheduled_task(
    name: String,
    prompt: String,
    schedule_type: String,
    schedule_value: String,
    store: State<'_, Arc<TaskStore>>,
) -> Result<ScheduledTask, String> {
    let schedule = parse_schedule(&schedule_type, &schedule_value).map_err(|e| e.to_string())?;
    store
        .create(TaskSource::User, &name, &prompt, schedule)
        .map_err(|e| e.to_string())
}

/// 列出全部定时任务（创建时间倒序）
#[tauri::command]
pub fn list_scheduled_tasks(
    store: State<'_, Arc<TaskStore>>,
) -> Result<Vec<ScheduledTask>, String> {
    Ok(store.list())
}

/// 删除定时任务
#[tauri::command]
pub fn delete_scheduled_task(
    task_id: String,
    store: State<'_, Arc<TaskStore>>,
) -> Result<(), String> {
    store.delete(&task_id).map_err(|e| e.to_string())
}

/// 停用 / 启用定时任务（不删除）
#[tauri::command]
pub fn toggle_scheduled_task(
    task_id: String,
    enabled: bool,
    store: State<'_, Arc<TaskStore>>,
) -> Result<(), String> {
    store.toggle(&task_id, enabled).map_err(|e| e.to_string())
}

/// 编辑定时任务定义（名称/内容/触发方式），id 不变，执行历史保留
#[tauri::command]
pub fn update_scheduled_task(
    task_id: String,
    name: String,
    prompt: String,
    schedule_type: String,
    schedule_value: String,
    store: State<'_, Arc<TaskStore>>,
) -> Result<ScheduledTask, String> {
    let schedule = parse_schedule(&schedule_type, &schedule_value).map_err(|e| e.to_string())?;
    store
        .update(&task_id, &name, &prompt, schedule)
        .map_err(|e| e.to_string())
}

/// 清除某任务的未读标记（前端查看执行历史后调用）
#[tauri::command]
pub fn mark_task_read(task_id: String, store: State<'_, Arc<TaskStore>>) -> Result<(), String> {
    store.mark_read(&task_id).map_err(|e| e.to_string())
}

/// 全部任务的未读执行总数（驱动侧边栏角标）
#[tauri::command]
pub fn get_unread_task_count(store: State<'_, Arc<TaskStore>>) -> Result<usize, String> {
    Ok(store.unread_count())
}

// ============ 渠道对接（飞书 / QQ） ============

/// 启动飞书渠道。读取最新配置，已在运行则先停后启（改完配置直接调它即可生效）
#[tauri::command]
pub fn feishu_start(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    channels.start_feishu(config.get().feishu, engine.inner().clone())
}

/// 停止飞书渠道
#[tauri::command]
pub fn feishu_stop(channels: State<'_, Arc<ChannelManager>>) -> Result<(), String> {
    channels.stop(ChannelKind::Feishu);
    Ok(())
}

/// 查询飞书渠道当前状态
#[tauri::command]
pub fn feishu_status(channels: State<'_, Arc<ChannelManager>>) -> Result<ChannelStatus, String> {
    Ok(channels.status(ChannelKind::Feishu))
}

/// 启动 QQ 渠道。读取最新配置，已在运行则先停后启
#[tauri::command]
pub fn qq_start(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
    engine: State<'_, Arc<Engine>>,
) -> Result<(), String> {
    channels.start_qq(config.get().qq, engine.inner().clone())
}

/// 停止 QQ 渠道
#[tauri::command]
pub fn qq_stop(channels: State<'_, Arc<ChannelManager>>) -> Result<(), String> {
    channels.stop(ChannelKind::Qq);
    Ok(())
}

/// 查询 QQ 渠道当前状态
#[tauri::command]
pub fn qq_status(channels: State<'_, Arc<ChannelManager>>) -> Result<ChannelStatus, String> {
    Ok(channels.status(ChannelKind::Qq))
}

/// 测试 QQ 凭证：取一次 token 和网关地址，不建立长连接
#[tauri::command]
pub async fn qq_test_connection(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
) -> Result<(), String> {
    let cfg = config.get().qq;
    if cfg.official.app_id.is_empty() || cfg.official.app_secret.is_empty() {
        return Err("请先填写 AppID 与 AppSecret".to_string());
    }

    let token = Arc::new(crate::channel::qq::official::TokenProvider::new(
        channels.http().clone(),
        cfg.official.token_url(),
        &cfg.official.app_id,
        &cfg.official.app_secret,
    ));
    let api = crate::channel::qq::official::QqApi::new(
        channels.http().clone(),
        cfg.official.api_base(),
        token,
    );

    crate::channel::qq::official::test_credentials(&api)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// 测试凭证：只做一次端点协商，不建立长连接，也不影响正在运行的渠道
#[tauri::command]
pub async fn feishu_test_connection(
    channels: State<'_, Arc<ChannelManager>>,
    config: State<'_, ConfigStore>,
) -> Result<(), String> {
    let cfg = config.get().feishu;
    if cfg.app_id.is_empty() || cfg.app_secret.is_empty() {
        return Err("请先填写 app_id 与 app_secret".to_string());
    }

    crate::channel::feishu::test_credentials(
        channels.http(),
        cfg.base_url(),
        &cfg.app_id,
        &cfg.app_secret,
    )
    .await
    .map_err(|e| format!("{e:#}"))
}

// ============ 渠道通讯录 ============

/// 通讯录列表：total 是全量人数，users 是本次匹配结果，owners 按渠道索引
#[derive(serde::Serialize)]
pub struct ContactList {
    pub total: usize,
    pub users: Vec<Contact>,
    pub owners: BTreeMap<String, OwnerContact>,
}

/// 列出通讯录（前端主人管理页用）。query 对 id / name / profile 模糊匹配。
/// owners 不受 query 影响，始终返回全部已记录的主人
#[tauri::command]
pub fn list_contacts(
    query: Option<String>,
    limit: Option<usize>,
    contacts: State<'_, Arc<ContactStore>>,
) -> Result<ContactList, String> {
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    Ok(ContactList {
        total: contacts.len(),
        users: contacts.list(query.as_deref(), limit),
        owners: contacts.owners(),
    })
}

/// 手工更正某渠道主人（前端通讯录页）。id 必填，昵称可空；
/// 通常由渠道首次私聊自动记录，这里只用于纠正
#[tauri::command]
pub fn update_owner(
    channel: String,
    id: String,
    name: Option<String>,
    contacts: State<'_, Arc<ContactStore>>,
) -> Result<OwnerContact, String> {
    contacts
        .set_owner(&channel, &id, name.as_deref().unwrap_or(""))
        .map_err(|e| format!("{e:#}"))
}

/// 更新通讯录条目。profile 是整体替换；name / profile 至少提供一个
#[tauri::command]
pub fn update_contact(
    id: String,
    name: Option<String>,
    profile: Option<String>,
    contacts: State<'_, Arc<ContactStore>>,
) -> Result<Contact, String> {
    contacts
        .update(&id, name.as_deref(), profile.as_deref())
        .map_err(|e| format!("{e:#}"))
}

/// 删除通讯录条目。只供前端主人管理，智能体工具没有删除权限
#[tauri::command]
pub fn delete_contact(id: String, contacts: State<'_, Arc<ContactStore>>) -> Result<(), String> {
    contacts.delete(&id).map_err(|e| format!("{e:#}"))
}
