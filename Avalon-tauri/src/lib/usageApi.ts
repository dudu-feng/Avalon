// 后端用量统计命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { invoke } from '@tauri-apps/api/core';
import type { DailyUsageRow } from '../types/usage';

/** 查询最近 N 天用量（按「天 × 模型」展平），供首页控制台报表读取 */
export async function queryDailyUsage(days: number): Promise<DailyUsageRow[]> {
  return invoke<DailyUsageRow[]>('query_daily_usage', { days });
}
