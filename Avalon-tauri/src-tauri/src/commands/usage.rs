// IPC：用量统计查询。

use std::sync::Arc;

use tauri::State;

use crate::usage::{DailyUsageRow, UsageStore};

/// 查询最近 N 天用量（按「天 × 模型」展平），供首页控制台报表读取
#[tauri::command]
pub fn query_daily_usage(
    days: usize,
    usage: State<'_, Arc<UsageStore>>,
) -> Result<Vec<DailyUsageRow>, String> {
    Ok(usage.query_daily(days))
}
