// scheduler 定时任务模块
//
// 定位：定时任务 = 自动驱动的会话（channel = 任务 id），由心跳触发 engine.run 跑一轮 ReAct，
// 执行过程复用 session 存储与前端消息渲染。分层：L2 静默层（任务执行）+ L3 心跳层（零 token 调度）。
// 数据模型 + 到期判断（is_due）+ 调度（Scheduler）+ 持久化（TaskStore）。

pub mod scheduler;
pub mod store;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

pub use scheduler::Scheduler;
pub use store::TaskStore;

/// 触发时间（结构化，不裸 cron：不支持比「每天」更细的粒度，天然防 agent 创建高频率任务）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleType {
    /// 一次性：具体时刻 "YYYY-MM-DD HH:MM"（或带 :SS）
    Once { at: String },
    /// 每天：时刻 "HH:MM"
    Daily { time: String },
    /// 每周：weekday（1=周一..7=周日）+ 时刻 "HH:MM"
    Weekly { weekday: u32, time: String },
}

/// 任务来源（UI 展示 + 护栏：agent 创建受数量上限约束）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSource {
    User,
    Agent,
}

/// 执行状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    Failed,
}

/// 每次执行的轻量元数据（完整消息走 session，这里只留索引，驱动卡片摘要 + 未读角标）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunMeta {
    pub triggered_at: String,
    pub status: RunStatus,
    #[serde(default)]
    pub read: bool,
}

/// 定时任务定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// 任务 id，同时作为会话 channel（task_ 前缀，下划线安全，避开 Windows 文件名非法字符）
    pub id: String,
    pub source: TaskSource,
    /// 任务名称（卡片标题，简短）
    pub name: String,
    /// 任务内容 = 每次触发喂给 agent 的输入
    pub prompt: String,
    pub schedule: ScheduleType,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: String,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub runs: Vec<TaskRunMeta>,
}

fn default_true() -> bool {
    true
}

impl ScheduledTask {
    /// 会话 channel = 任务 id（与用户 channel 天然隔离）
    pub fn channel(&self) -> &str {
        &self.id
    }

    /// 会话标题（id 不变，title 用「定时任务-任务名称」让会话文件可读）
    pub fn session_title(&self) -> String {
        format!("定时任务-{}", self.name)
    }
}

/// 解析前端 / 工具传入的 schedule_type + schedule_value → ScheduleType。
/// 参数扁平化（type + value 两个字符串），降低 LLM 填错概率、普通用户免手写 cron。
pub fn parse_schedule(schedule_type: &str, schedule_value: &str) -> Result<ScheduleType> {
    let v = schedule_value.trim();
    match schedule_type.trim() {
        "once" => {
            parse_datetime(v).ok_or_else(|| anyhow!("once 时间格式应为 YYYY-MM-DD HH:MM"))?;
            Ok(ScheduleType::Once { at: v.to_string() })
        }
        "daily" => {
            parse_time(v).ok_or_else(|| anyhow!("daily 时间格式应为 HH:MM"))?;
            Ok(ScheduleType::Daily { time: v.to_string() })
        }
        "weekly" => {
            let (w, t) = v
                .split_once(' ')
                .ok_or_else(|| anyhow!("weekly 格式应为 'N HH:MM'（N=1 周一 .. 7 周日）"))?;
            let weekday: u32 = w
                .trim()
                .parse()
                .ok()
                .filter(|n| (1..=7).contains(n))
                .ok_or_else(|| anyhow!("weekly 星期应为 1(周一)..7(周日)"))?;
            let time = t.trim();
            parse_time(time).ok_or_else(|| anyhow!("weekly 时间格式应为 HH:MM"))?;
            Ok(ScheduleType::Weekly {
                weekday,
                time: time.to_string(),
            })
        }
        other => Err(anyhow!(
            "schedule_type 应为 once / daily / weekly，收到 '{other}'"
        )),
    }
}

/// 判断任务此刻是否到期（启用 + 已到触发时刻 + 本周期尚未执行）
pub fn is_due(task: &ScheduledTask, now: DateTime<Local>) -> bool {
    if !task.enabled {
        return false;
    }
    let today = now.format("%Y-%m-%d").to_string();
    match &task.schedule {
        ScheduleType::Once { at } => match parse_datetime(at) {
            Some(at_dt) => now >= at_dt && task.last_run_at.is_none(),
            None => false,
        },
        ScheduleType::Daily { time } => match parse_today_time(&today, time) {
            Some(due) => now >= due && last_run_date(task) != Some(today),
            None => false,
        },
        ScheduleType::Weekly { weekday, time } => {
            if now.weekday().number_from_monday() != *weekday {
                return false;
            }
            match parse_today_time(&today, time) {
                Some(due) => now >= due && last_run_date(task) != Some(today),
                None => false,
            }
        }
    }
}

/// 上次执行日期（last_run_at 前 10 字符 = YYYY-MM-DD）
fn last_run_date(task: &ScheduledTask) -> Option<String> {
    task.last_run_at
        .as_ref()
        .map(|s| s.chars().take(10).collect())
}

fn parse_datetime(s: &str) -> Option<DateTime<Local>> {
    let s = s.trim();
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return naive.and_local_timezone(Local).single();
        }
    }
    // 仅日期：当作当天 00:00
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|n| n.and_local_timezone(Local).single())
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn parse_today_time(today: &str, time: &str) -> Option<DateTime<Local>> {
    let date = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    let t = parse_time(time)?;
    date.and_hms_opt(t.hour(), t.minute(), 0)
        .and_then(|n| n.and_local_timezone(Local).single())
}
