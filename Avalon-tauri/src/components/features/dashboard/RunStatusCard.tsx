// 运行状态：反映「此刻」的助手状态，不随报表区间变化。

import { Card, CircleProgress } from '../../ui';
import type { DashboardRunStatus } from '../../../types/dashboard';
import { formatFull, formatPercent } from './dashboardData';
import styles from './RunStatusCard.module.css';

export interface RunStatusCardProps {
  status: DashboardRunStatus;
}

export function RunStatusCard({ status }: RunStatusCardProps) {
  const { contextUsage, contextRatio, activeModel, activeModelName } = status;

  return (
    <Card as="section" className={styles.card}>
      <header className={styles.head}>
        <h3 className={styles.title}>运行状态</h3>
        <p className={styles.sub}>当前会话与配置概况</p>
      </header>

      <div className={styles.context}>
        <CircleProgress
          value={contextUsage?.used_tokens ?? 0}
          max={contextUsage?.threshold || 1}
          size={64}
          strokeWidth={5}
          label={formatPercent(contextRatio, 0)}
        />
        <div className={styles.contextText}>
          <p className={styles.contextLabel}>上下文占用</p>
          <p className={styles.contextValue}>
            {contextUsage
              ? `${formatFull(contextUsage.used_tokens)} / ${formatFull(contextUsage.threshold)} token`
              : '暂无数据'}
          </p>
          <p className={styles.contextHint}>达到阈值后自动压缩</p>
        </div>
      </div>

      <dl className={styles.list}>
        <div className={styles.entry}>
          <dt className={styles.term}>活跃模型</dt>
          <dd className={styles.desc} title={activeModelName ?? undefined}>
            {activeModel ?? '—'}
          </dd>
        </div>
        <div className={styles.entry}>
          <dt className={styles.term}>已配置模型</dt>
          <dd className={styles.desc}>{status.modelCount}</dd>
        </div>
        <div className={styles.entry}>
          <dt className={styles.term}>会话总数</dt>
          <dd className={styles.desc}>{status.sessionCount}</dd>
        </div>
        <div className={styles.entry}>
          <dt className={styles.term}>启用任务</dt>
          <dd className={styles.desc}>
            {status.enabledTaskCount} / {status.totalTaskCount}
          </dd>
        </div>
      </dl>
    </Card>
  );
}
