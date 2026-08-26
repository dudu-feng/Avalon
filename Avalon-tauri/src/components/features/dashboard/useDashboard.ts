// 仪表盘取数编排。
//
// 状态由仪表盘页独占（App 单页切换，卸载即销毁），故走页面级 hook 路线：
// useState + 命名布尔 loading + try/catch 吞错 + 卸载防护，对齐 features/chat/useChat.ts。
//
// 数据分两组：
// - 随区间变化：用量（为算环比取双倍天数）
// - 不随区间变化：会话 / 上下文 / 配置 / 任务（挂载时取一次）
// 切区间只重取用量，且不清空旧数据 —— 否则会闪骨架屏。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { DEFAULT_CHANNEL, getContextUsage, listSessions } from '../../../lib/chatApi';
import { listScheduledTasks, onTaskFinished } from '../../../lib/schedulerApi';
import { getConfig } from '../../../lib/settingsApi';
import { queryDailyUsage } from '../../../lib/usageApi';
import type { ContextUsage, SessionMeta } from '../../../types/chat';
import type { AppConfig } from '../../../types/config';
import type {
  DashboardKpis,
  DashboardRange,
  DashboardRunStatus,
  ModelUsage,
  TrendPoint,
} from '../../../types/dashboard';
import type { ScheduledTask } from '../../../types/scheduler';
import type { DailyUsageRow } from '../../../types/usage';
import { computeDashboard } from './dashboardData';

/** 取数模式：首载置 loading / 切区间置 refreshing / 后台事件静默刷新 */
type FetchMode = 'initial' | 'refresh' | 'silent';

export interface UseDashboardResult {
  kpis: DashboardKpis;
  trends: TrendPoint[];
  modelRanking: ModelUsage[];
  runStatus: DashboardRunStatus;
  /** 供表格视图读取的原始行（已按当前区间过滤前的全量，表格自行裁剪） */
  rows: DailyUsageRow[];

  rangeDays: DashboardRange;
  setRangeDays: (d: DashboardRange) => void;
  refresh: () => void;

  /** 首载中（尚无任何数据），此时才显示骨架 */
  loading: boolean;
  /** 切区间重取中，保留旧渲染、降低透明度 */
  refreshing: boolean;
}

export function useDashboard(): UseDashboardResult {
  const [rangeDays, setRangeDays] = useState<DashboardRange>(7);

  const [rows, setRows] = useState<DailyUsageRow[]>([]);
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [contextUsage, setContextUsage] = useState<ContextUsage | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [tasks, setTasks] = useState<ScheduledTask[]>([]);

  const [usageLoaded, setUsageLoaded] = useState(false);
  const [staticLoaded, setStaticLoaded] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  // 卸载防护：所有异步回调落盘前检查
  const aliveRef = useRef(true);
  // 区间镜像：事件回调里必须读它，闭包里的 rangeDays 会是订阅时的旧值
  const rangeRef = useRef<DashboardRange>(rangeDays);
  // 是否已完成首载，决定下次取数走 initial 还是 refresh 分支
  const usageLoadedRef = useRef(false);
  // 请求序号：快速连点区间时，只有最后发起的那次允许落盘
  const seqRef = useRef(0);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  useEffect(() => {
    rangeRef.current = rangeDays;
  }, [rangeDays]);

  const fetchUsage = useCallback(async (days: DashboardRange, mode: FetchMode) => {
    const seq = ++seqRef.current;
    if (mode === 'refresh') setRefreshing(true);
    try {
      // 取双倍天数：后一半是当前窗口，前一半用于算环比
      const data = await queryDailyUsage(days * 2);
      if (!aliveRef.current || seq !== seqRef.current) return;
      setRows(data);
    } catch (e) {
      console.error('query_daily_usage 失败:', e);
    } finally {
      if (aliveRef.current && seq === seqRef.current) {
        // 失败也要解除首载，否则骨架屏永远不散
        if (mode === 'initial') {
          usageLoadedRef.current = true;
          setUsageLoaded(true);
        }
        if (mode === 'refresh') setRefreshing(false);
      }
    }
  }, []);

  const fetchTasks = useCallback(async () => {
    try {
      const data = await listScheduledTasks();
      if (aliveRef.current) setTasks(data);
    } catch (e) {
      console.error('list_scheduled_tasks 失败:', e);
    }
  }, []);

  // 静态四源：挂载取一次，彼此独立故并发
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [sess, ctx, cfg, tk] = await Promise.all([
          listSessions(DEFAULT_CHANNEL),
          getContextUsage(DEFAULT_CHANNEL),
          getConfig(),
          listScheduledTasks(),
        ]);
        if (cancelled) return;
        setSessions(sess);
        setContextUsage(ctx);
        setConfig(cfg);
        setTasks(tk);
      } catch (e) {
        console.error('仪表盘静态数据加载失败:', e);
      } finally {
        if (!cancelled) setStaticLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 用量：首次走 initial，此后每次切区间走 refresh（不清旧数据）
  useEffect(() => {
    void fetchUsage(rangeDays, usageLoadedRef.current ? 'refresh' : 'initial');
  }, [rangeDays, fetchUsage]);

  // 任务跑完会产生新用量并追加 runs，两者都要刷；静默进行，避免图表抖动。
  // 任务跑在 task_ 渠道，不影响 app 渠道的上下文与会话，故不重取那两项。
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    onTaskFinished(() => {
      if (cancelled) return;
      void fetchUsage(rangeRef.current, 'silent');
      void fetchTasks();
    })
      .then((fn) => {
        // 订阅是异步的，返回前可能已卸载
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((e) => console.error('订阅 task-finished 失败:', e));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [fetchUsage, fetchTasks]);

  const refresh = useCallback(() => {
    void fetchUsage(rangeRef.current, 'refresh');
    void fetchTasks();
  }, [fetchUsage, fetchTasks]);

  const snapshot = useMemo(
    () => computeDashboard({ rows, sessions, contextUsage, config, tasks, rangeDays }),
    [rows, sessions, contextUsage, config, tasks, rangeDays],
  );

  return {
    kpis: snapshot.kpis,
    trends: snapshot.trends,
    modelRanking: snapshot.modelRanking,
    runStatus: snapshot.runStatus,
    rows,
    rangeDays,
    setRangeDays,
    refresh,
    loading: !usageLoaded || !staticLoaded,
    refreshing,
  };
}
