// 仪表盘数据派生：纯函数，无 React 依赖。
//
// 后端 query_daily_usage 返回的是「天 × 模型」展平的多行结构：同一天用了两个模型
// 就有两行，某天没用则完全没有行。所以这里要做两件事：先跨模型按天合并，再按
// 显式日期轴补齐空缺为 0。
//
// 环比：取双倍天数的数据，用两条日期轴（当前窗口 / 上一窗口）分别求和后对比。

import type { DailyUsageRow } from '../../../types/usage';
import type { SessionMeta, ContextUsage } from '../../../types/chat';
import type { ScheduledTask } from '../../../types/scheduler';
import type { AppConfig } from '../../../types/config';
import type {
  DashboardKpi,
  DashboardKpis,
  DashboardRunStatus,
  DashboardSnapshot,
  ModelUsage,
  TrendPoint,
} from '../../../types/dashboard';

/** 某天跨模型合并后的累计值 */
interface DateAgg {
  input: number;
  output: number;
  total: number;
  cached: number;
  requests: number;
}

/**
 * Date → "YYYY-MM-DD"（本地时区）。
 *
 * 必须走 getFullYear/getMonth/getDate，不能用 toISOString().slice(0,10)：
 * 后者按 UTC 取日期，在东八区凌晨 0–8 点会把「今天」算成昨天，
 * 导致整条趋势轴与后端 date 字段错位一天。
 */
export function localDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${y}-${m}-${day}`;
}

/**
 * 构造一条升序日期轴：从「今天 - offsetDays」往回数 range 天（含首尾）。
 * offsetDays = 0 得当前窗口，offsetDays = range 得上一窗口。
 *
 * 用 new Date(y, m, d - n) 本地构造，跨月/跨年/闰年由 Date 自动归一化。
 */
export function buildDateRange(range: number, offsetDays: number): string[] {
  const out: string[] = [];
  const today = new Date();
  const y = today.getFullYear();
  const m = today.getMonth();
  const d = today.getDate();
  for (let i = range - 1; i >= 0; i--) {
    out.push(localDate(new Date(y, m, d - offsetDays - i)));
  }
  return out;
}

/** 跨模型按天合并（同一天的多行先加起来，否则补齐时只会取到其中一行） */
export function aggregateByDate(rows: DailyUsageRow[]): Map<string, DateAgg> {
  const map = new Map<string, DateAgg>();
  for (const r of rows) {
    const a = map.get(r.date) ?? { input: 0, output: 0, total: 0, cached: 0, requests: 0 };
    a.input += r.input_tokens;
    a.output += r.output_tokens;
    a.total += r.total_tokens;
    a.cached += r.cached_tokens;
    a.requests += r.requests;
    map.set(r.date, a);
  }
  return map;
}

/** 沿日期轴求和（轴上没有数据的日期按 0 计） */
function sumWindow(agg: Map<string, DateAgg>, axis: string[]): DateAgg {
  const sum: DateAgg = { input: 0, output: 0, total: 0, cached: 0, requests: 0 };
  for (const date of axis) {
    const a = agg.get(date);
    if (!a) continue;
    sum.input += a.input;
    sum.output += a.output;
    sum.total += a.total;
    sum.cached += a.cached;
    sum.requests += a.requests;
  }
  return sum;
}

/** 组装单个 KPI：上期为 0 或任一侧为 null 时，deltaRatio 返回 null 而非 Infinity */
export function makeKpi(current: number | null, previous: number | null): DashboardKpi {
  const delta = current !== null && previous !== null ? current - previous : null;
  const deltaRatio =
    delta !== null && previous !== null && previous !== 0 ? delta / previous : null;
  return { current, previous, delta, deltaRatio };
}

/**
 * 按模型聚合（降序）。
 *
 * rows 里含双倍天数的数据（为算环比而取），必须用当前窗口的日期集合过滤，
 * 否则上期用量会被算进本期的模型分布。
 */
export function aggregateByModel(rows: DailyUsageRow[], dates: Set<string>): ModelUsage[] {
  const map = new Map<string, ModelUsage>();
  for (const r of rows) {
    if (!dates.has(r.date)) continue;
    const m = map.get(r.model) ?? { model: r.model, totalTokens: 0, requests: 0 };
    m.totalTokens += r.total_tokens;
    m.requests += r.requests;
    map.set(r.model, m);
  }
  return [...map.values()].sort((a, b) => b.totalTokens - a.totalTokens);
}

/** 汇总全部任务的执行成败计数 */
function countTaskRuns(tasks: ScheduledTask[]): { success: number; failure: number } {
  let success = 0;
  let failure = 0;
  for (const t of tasks) {
    for (const r of t.runs) {
      if (r.status === 'succeeded') success++;
      else failure++;
    }
  }
  return { success, failure };
}

export interface ComputeInput {
  rows: DailyUsageRow[];
  sessions: SessionMeta[];
  contextUsage: ContextUsage | null;
  config: AppConfig | null;
  tasks: ScheduledTask[];
  rangeDays: number;
}

/** 由原始数据派生出仪表盘的全部展示指标 */
export function computeDashboard(input: ComputeInput): DashboardSnapshot {
  const { rows, sessions, contextUsage, config, tasks, rangeDays } = input;

  const currentAxis = buildDateRange(rangeDays, 0);
  const previousAxis = buildDateRange(rangeDays, rangeDays);
  const agg = aggregateByDate(rows);

  const cur = sumWindow(agg, currentAxis);
  const prev = sumWindow(agg, previousAxis);

  // 命中率与均次消耗都用「区间总和相除」而非「按天求平均」，
  // 这样高流量的日子权重才是对的。
  const kpis: DashboardKpis = {
    totalTokens: makeKpi(cur.total, prev.total),
    requests: makeKpi(cur.requests, prev.requests),
    cacheHitRate: makeKpi(
      cur.input > 0 ? cur.cached / cur.input : null,
      prev.input > 0 ? prev.cached / prev.input : null,
    ),
    avgTokensPerRequest: makeKpi(
      cur.requests > 0 ? cur.total / cur.requests : null,
      prev.requests > 0 ? prev.total / prev.requests : null,
    ),
  };

  const trends: TrendPoint[] = currentAxis.map((date) => {
    const a = agg.get(date);
    return {
      date,
      inputTokens: a?.input ?? 0,
      outputTokens: a?.output ?? 0,
      totalTokens: a?.total ?? 0,
    };
  });

  const modelRanking = aggregateByModel(rows, new Set(currentAxis));

  const activeModel = config?.active_model ?? null;
  const { success, failure } = countTaskRuns(tasks);

  const runStatus: DashboardRunStatus = {
    contextUsage,
    contextRatio:
      contextUsage && contextUsage.threshold > 0
        ? contextUsage.used_tokens / contextUsage.threshold
        : null,
    activeModel,
    // active_model 可能指向已删除的模型，找不到时返回 null 由 UI 兜底
    activeModelName:
      config?.models.find((m) => m.name === activeModel)?.modelname ?? null,
    sessionCount: sessions.length,
    modelCount: config?.models.length ?? 0,
    enabledTaskCount: tasks.filter((t) => t.enabled).length,
    totalTaskCount: tasks.length,
    taskSuccessCount: success,
    taskFailureCount: failure,
  };

  return { kpis, trends, modelRanking, runStatus };
}

/** 空快照：首屏未取到数据时占位，保证 UI 不用处理 undefined */
export function emptySnapshot(rangeDays: number): DashboardSnapshot {
  return computeDashboard({
    rows: [],
    sessions: [],
    contextUsage: null,
    config: null,
    tasks: [],
    rangeDays,
  });
}

// ============ 展示格式化（纯函数，供各卡片复用）============

/** 紧凑数字：1284 → "1,284"；12900 → "12.9K"；4200000 → "4.2M" */
export function formatCompact(n: number | null): string {
  if (n === null || !Number.isFinite(n)) return '—';
  const abs = Math.abs(n);
  if (abs >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (abs >= 10_000) return `${(n / 1000).toFixed(1)}K`;
  return Math.round(n).toLocaleString('en-US');
}

/** 完整千分位，用于表格与 tooltip 的精确读数 */
export function formatFull(n: number | null): string {
  if (n === null || !Number.isFinite(n)) return '—';
  return Math.round(n).toLocaleString('en-US');
}

/** 比值 → 百分比文本，如 0.612 → "61.2%" */
export function formatPercent(ratio: number | null, digits = 1): string {
  if (ratio === null || !Number.isFinite(ratio)) return '—';
  return `${(ratio * 100).toFixed(digits)}%`;
}

/** "2026-08-20" → "8/20"（轴标签用短格式） */
export function formatShortDate(date: string): string {
  const parts = date.split('-');
  if (parts.length !== 3) return date;
  return `${Number(parts[1])}/${Number(parts[2])}`;
}
