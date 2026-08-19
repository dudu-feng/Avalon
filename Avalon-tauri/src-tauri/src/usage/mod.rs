// 用量统计模块
//
// 独立于会话生命周期的 token 聚合层：每次对话收尾时按「天 × 模型」增量累加，
// 供首页控制台报表按模型/日期查询消耗。设计要点（见 doc/v0.2/03-usage-stats-module.md）：
//   - 累计值常驻内存（构造时读一次文件），record_usage 只累加不读文件；
//   - 即时原子写（tmp + rename），强杀进程最多丢最后一次，攒内存等退出则丢整段运行期；
//   - 旁路失败静默：record_usage 失败只记日志，绝不把 chat 主流程带崩。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::llm::TokenUsage;

/// 单个模型在某天的累计用量（u64 累计，避免长期溢出）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    #[serde(default)]
    pub requests: u64,
}

/// 报表查询返回的一行（展平 date + model + 用量），前端直接映射表格/图
#[derive(Debug, Clone, Serialize)]
pub struct DailyUsageRow {
    pub date: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub reasoning_tokens: u64,
    pub cached_tokens: u64,
    pub requests: u64,
}

/// usage.json 磁盘结构：顶层 daily 包装（为将来扩展非 daily 统计留空间）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct UsageFile {
    #[serde(default)]
    daily: HashMap<String, HashMap<String, ModelUsage>>,
}

/// 用量统计存储：内存累计 + 即时原子写
pub struct UsageStore {
    path: PathBuf,
    file: Mutex<UsageFile>,
}

impl UsageStore {
    /// 构造：读一次文件进内存；文件不存在/损坏 → 空 map 静默降级（不影响启动）
    pub fn new(path: PathBuf) -> Self {
        let file = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<UsageFile>(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            file: Mutex::new(file),
        }
    }

    /// 收尾调用：内存累加 + 原子写（实时落盘）
    pub fn record_usage(&self, model: &str, usage: &TokenUsage) -> Result<()> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.record_usage_on(model, usage, &today)
    }

    /// 按指定日期累加（抽出日期参数，便于单测覆盖跨天分桶）
    pub(crate) fn record_usage_on(&self, model: &str, usage: &TokenUsage, date: &str) -> Result<()> {
        let mut file = self.file.lock().unwrap();
        let e = file
            .daily
            .entry(date.to_string())
            .or_default()
            .entry(model.to_string())
            .or_default();
        e.input_tokens += usage.input_tokens as u64;
        e.output_tokens += usage.output_tokens as u64;
        e.total_tokens += usage.total_tokens as u64;
        e.reasoning_tokens += usage.reasoning_tokens as u64;
        e.cached_tokens += usage.cached_tokens as u64;
        e.requests += 1;
        self.write_locked(&file)
    }

    /// 报表读取：直接读内存，返回最近 days 天（日期升序、同天按模型名升序，无数据的天跳过）
    pub fn query_daily(&self, days: usize) -> Vec<DailyUsageRow> {
        let file = self.file.lock().unwrap();
        let mut dates: Vec<&String> = file.daily.keys().collect();
        dates.sort();
        // %Y-%m-%d 字典序即时间序，取最近 days 天
        let start = dates.len().saturating_sub(days);
        let mut rows = Vec::new();
        for i in start..dates.len() {
            let date = dates[i];
            if let Some(models) = file.daily.get(date) {
                let mut names: Vec<&String> = models.keys().collect();
                names.sort();
                for name in names {
                    let u = &models[name];
                    rows.push(DailyUsageRow {
                        date: date.clone(),
                        model: name.clone(),
                        input_tokens: u.input_tokens,
                        output_tokens: u.output_tokens,
                        total_tokens: u.total_tokens,
                        reasoning_tokens: u.reasoning_tokens,
                        cached_tokens: u.cached_tokens,
                        requests: u.requests,
                    });
                }
            }
        }
        rows
    }

    /// 锁内序列化 + 原子写（tmp + rename，自动建目录；与 session store 同款）
    fn write_locked(&self, file: &UsageFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建用量统计目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(file).context("序列化用量统计失败")?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("写入用量统计临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("原子替换用量统计文件失败: {}", self.path.display()))?;
        Ok(())
    }
}
