// 定时任务执行统计（全部任务的累计成败，非区间指标）。
//
// 成功/失败是真正的状态语义，所以这里用保留的 --success / --error，
// 且始终配文字标签 —— 状态永远不能只靠颜色表达。

import { Card, ProgressBar } from '../../ui';
import type { DashboardRunStatus } from '../../../types/dashboard';
import { formatPercent } from './dashboardData';
import styles from './TaskStatsCard.module.css';

export interface TaskStatsCardProps {
  status: DashboardRunStatus;
}

export function TaskStatsCard({ status }: TaskStatsCardProps) {
  const { taskSuccessCount: success, taskFailureCount: failure } = status;
  const total = success + failure;
  const rate = total > 0 ? success / total : null;

  return (
    <Card as="section">
      <header className={styles.head}>
        <div>
          <h3 className={styles.title}>任务执行</h3>
          <p className={styles.sub}>全部定时任务的累计执行情况</p>
        </div>
        {total > 0 && (
          <p className={styles.rate}>
            <span className={styles.rateValue}>{formatPercent(rate)}</span>
            <span className={styles.rateLabel}>成功率</span>
          </p>
        )}
      </header>

      {total === 0 ? (
        <p className={styles.empty}>
          {status.totalTaskCount === 0 ? '还没有创建定时任务' : '任务尚未执行过'}
        </p>
      ) : (
        <>
          <ProgressBar value={success} max={total} className={styles.bar} />
          <ul className={styles.legend}>
            <li className={styles.legendItem}>
              <span className={`${styles.dot} ${styles.dotSuccess}`} aria-hidden="true" />
              成功
              <span className={styles.count}>{success}</span>
            </li>
            <li className={styles.legendItem}>
              <span className={`${styles.dot} ${styles.dotFailure}`} aria-hidden="true" />
              失败
              <span className={styles.count}>{failure}</span>
            </li>
            <li className={styles.legendItem}>
              总执行
              <span className={styles.count}>{total}</span>
            </li>
          </ul>
        </>
      )}
    </Card>
  );
}
