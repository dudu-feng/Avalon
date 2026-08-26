// 仪表盘：token 用量与运行状态的数据看板。
//
// 数据全部由既有 Tauri 命令组合而来，后端零改动。
// 区间选择器只有一处，统一作用于下方所有图表 —— 不给每张卡片各配一个筛选器。

import { PageContainer, Skeleton, StatTile, Tabs } from '../../components/ui';
import type { StatTileProps } from '../../components/ui';
import {
  ModelRankCard,
  RunStatusCard,
  TaskStatsCard,
  UsageTrendCard,
  formatCompact,
  formatPercent,
  useDashboard,
} from '../../components/features/dashboard';
import type { DashboardKpi, DashboardRange } from '../../types/dashboard';
import styles from './DashboardPage.module.css';

const RANGE_OPTIONS = [
  { value: '7', label: '7 天' },
  { value: '14', label: '14 天' },
  { value: '30', label: '30 天' },
];

type DeltaProps = Pick<StatTileProps, 'delta' | 'deltaDirection'>;

function direction(v: number): NonNullable<StatTileProps['deltaDirection']> {
  if (v > 0) return 'up';
  if (v < 0) return 'down';
  return 'flat';
}

/**
 * 相对变化（百分比）。上期为 0 时 deltaRatio 为 null，此时整行环比不显示 ——
 * 「较上期 +∞%」没有任何信息量。
 */
function ratioDelta(kpi: DashboardKpi): DeltaProps {
  if (kpi.deltaRatio === null) return {};
  return {
    delta: formatPercent(Math.abs(kpi.deltaRatio)),
    deltaDirection: direction(kpi.deltaRatio),
  };
}

/** 绝对变化（百分点），用于本身就是比率的指标 —— 比率的相对变化会让人误读 */
function pointDelta(kpi: DashboardKpi): DeltaProps {
  if (kpi.delta === null) return {};
  return {
    delta: `${Math.abs(kpi.delta * 100).toFixed(1)} pt`,
    deltaDirection: direction(kpi.delta),
  };
}

export function DashboardPage() {
  const {
    kpis,
    trends,
    modelRanking,
    runStatus,
    rangeDays,
    setRangeDays,
    loading,
    refreshing,
  } = useDashboard();

  if (loading) {
    return (
      <PageContainer title="仪表盘" description="用量与运行状态总览">
        <div className={styles.kpiRow}>
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className={styles.skeletonTile} />
          ))}
        </div>
        <Skeleton className={styles.skeletonChart} />
      </PageContainer>
    );
  }

  return (
    <PageContainer title="仪表盘" description="用量与运行状态总览">
      <div className={styles.toolbar}>
        <Tabs
          options={RANGE_OPTIONS}
          value={String(rangeDays)}
          onChange={(v) => setRangeDays(Number(v) as DashboardRange)}
        />
      </div>

      {/* 切区间时保留上一次渲染、只降低透明度，避免骨架屏闪一下造成跳动 */}
      <div className={`${styles.content} ${refreshing ? styles.dimmed : ''}`}>
        <section className={styles.kpiRow}>
          <StatTile
            label="总 Token"
            value={formatCompact(kpis.totalTokens.current)}
            {...ratioDelta(kpis.totalTokens)}
          />
          <StatTile
            label="请求次数"
            value={formatCompact(kpis.requests.current)}
            {...ratioDelta(kpis.requests)}
          />
          <StatTile
            label="缓存命中率"
            value={formatPercent(kpis.cacheHitRate.current)}
            {...pointDelta(kpis.cacheHitRate)}
          />
          <StatTile
            label="平均每请求"
            value={formatCompact(kpis.avgTokensPerRequest.current)}
            {...ratioDelta(kpis.avgTokensPerRequest)}
          />
        </section>

        <UsageTrendCard trends={trends} rangeDays={rangeDays} />

        <div className={styles.twoCol}>
          <ModelRankCard ranking={modelRanking} />
          <RunStatusCard status={runStatus} />
        </div>

        <TaskStatsCard status={runStatus} />
      </div>
    </PageContainer>
  );
}
