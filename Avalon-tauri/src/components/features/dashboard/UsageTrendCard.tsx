// 主趋势图卡片：图例 + 堆叠柱 + 可展开的表格视图。
//
// 两个系列必须常驻图例 —— 不能让读者只靠颜色去猜身份。

import { useState } from 'react';
import { Card, StackedBarChart } from '../../ui';
import type { StackedBarDatum } from '../../ui';
import type { DashboardRange, TrendPoint } from '../../../types/dashboard';
import { formatCompact, formatShortDate } from './dashboardData';
import { UsageTable } from './UsageTable';
import styles from './UsageTrendCard.module.css';

export interface UsageTrendCardProps {
  trends: TrendPoint[];
  rangeDays: DashboardRange;
}

/** x 轴标签抽稀间隔：区间越长标得越疏，避免文字挤在一起 */
function labelStep(rangeDays: DashboardRange): number {
  if (rangeDays <= 7) return 1;
  if (rangeDays <= 14) return 2;
  return 5;
}

export function UsageTrendCard({ trends, rangeDays }: UsageTrendCardProps) {
  const [tableOpen, setTableOpen] = useState(false);

  const data: StackedBarDatum[] = trends.map((t) => ({
    key: t.date,
    label: formatShortDate(t.date),
    base: t.inputTokens,
    emphasis: t.outputTokens,
  }));

  return (
    <Card as="section">
      <header className={styles.head}>
        <div>
          <h3 className={styles.title}>Token 趋势</h3>
          <p className={styles.sub}>近 {rangeDays} 天，按日期堆叠</p>
        </div>
        <ul className={styles.legend}>
          <li className={styles.legendItem}>
            <span className={`${styles.swatch} ${styles.swatchEmphasis}`} aria-hidden="true" />
            输出
          </li>
          <li className={styles.legendItem}>
            <span className={`${styles.swatch} ${styles.swatchBase}`} aria-hidden="true" />
            输入
          </li>
        </ul>
      </header>

      <StackedBarChart
        data={data}
        baseName="输入"
        emphasisName="输出"
        height={216}
        formatValue={formatCompact}
        labelEvery={labelStep(rangeDays)}
        emptyText="这段时间还没有用量记录"
      />

      <button
        type="button"
        className={styles.toggle}
        onClick={() => setTableOpen((v) => !v)}
        aria-expanded={tableOpen}
      >
        <span className={`${styles.caret} ${tableOpen ? styles.caretOpen : ''}`} aria-hidden="true">
          ▸
        </span>
        {tableOpen ? '收起数据表格' : '查看数据表格'}
      </button>

      {tableOpen && <UsageTable trends={trends} />}
    </Card>
  );
}
