// 趋势图的表格视图。
//
// 存在的意义不是「补充信息」，而是无障碍要求：图表里的数值不能只能靠悬停读到，
// 必须有一条键盘与读屏可达的等价通道。列序与图表堆叠顺序一致（输出在前）。

import type { TrendPoint } from '../../../types/dashboard';
import { formatFull } from './dashboardData';
import styles from './UsageTable.module.css';

export interface UsageTableProps {
  trends: TrendPoint[];
}

export function UsageTable({ trends }: UsageTableProps) {
  return (
    <div className={styles.wrap}>
      <table className={styles.table}>
        <caption className={styles.caption}>按日期的 token 用量明细</caption>
        <thead>
          <tr>
            <th scope="col">日期</th>
            <th scope="col" className={styles.num}>
              输出
            </th>
            <th scope="col" className={styles.num}>
              输入
            </th>
            <th scope="col" className={styles.num}>
              合计
            </th>
          </tr>
        </thead>
        <tbody>
          {trends.map((t) => (
            <tr key={t.date}>
              <th scope="row" className={styles.date}>
                {t.date}
              </th>
              <td className={styles.num}>{formatFull(t.outputTokens)}</td>
              <td className={styles.num}>{formatFull(t.inputTokens)}</td>
              <td className={`${styles.num} ${styles.total}`}>{formatFull(t.totalTokens)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
