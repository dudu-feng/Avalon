// 仪表盘展示模型（camelCase，前端内部派生，无后端对应结构）
//
// 后端只提供「天 × 模型」展平的原始用量行（types/usage.ts），
// 这里的类型是 dashboardData.ts 聚合后的产物：补齐日期的趋势序列、
// 带环比的 KPI、模型排行、运行状态。

import type { ContextUsage } from './chat';

/** 报表时间区间（天） */
export type DashboardRange = 7 | 14 | 30;

/** 趋势序列的一个点：跨模型按天求和，空缺日已补 0，整体按日期升序 */
export interface TrendPoint {
  /** 本地日期，格式 "2026-08-20" */
  date: string;
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
}

/** 模型排行的一项：区间内该模型的累计用量 */
export interface ModelUsage {
  model: string;
  totalTokens: number;
  requests: number;
}

/**
 * 单个 KPI 指标。
 * current / previous 为 null 表示「分母为 0，无法计算」（如零请求时的均次消耗）。
 * deltaRatio 在上期为 0 或 null 时为 null —— 避免 Infinity / NaN 漏到 UI。
 */
export interface DashboardKpi {
  current: number | null;
  previous: number | null;
  /** current - previous */
  delta: number | null;
  /** delta / previous，上期为 0 时为 null */
  deltaRatio: number | null;
}

/** KPI 行的四个指标 */
export interface DashboardKpis {
  /** 区间总 token */
  totalTokens: DashboardKpi;
  /** 区间请求次数 */
  requests: DashboardKpi;
  /** 缓存命中率，比值 0..1（UI 侧乘 100 展示） */
  cacheHitRate: DashboardKpi;
  /** 平均每请求消耗 token */
  avgTokensPerRequest: DashboardKpi;
}

/** 运行状态（非区间指标，反映「此刻」的健康度） */
export interface DashboardRunStatus {
  contextUsage: ContextUsage | null;
  /** used / threshold；threshold <= 0 时为 null */
  contextRatio: number | null;
  /** 配置中的活跃模型名 */
  activeModel: string | null;
  /** 实际模型标识（models 里匹配 activeModel 的 modelname），找不到为 null */
  activeModelName: string | null;
  /** 会话总数（含活跃与归档） */
  sessionCount: number;
  /** 已配置的模型数 */
  modelCount: number;
  /** 启用中的定时任务数 */
  enabledTaskCount: number;
  /** 定时任务总数 */
  totalTaskCount: number;
  /** 全部任务累计执行成功次数 */
  taskSuccessCount: number;
  /** 全部任务累计执行失败次数 */
  taskFailureCount: number;
}

/** 一次完整派生结果 */
export interface DashboardSnapshot {
  kpis: DashboardKpis;
  trends: TrendPoint[];
  modelRanking: ModelUsage[];
  runStatus: DashboardRunStatus;
}
