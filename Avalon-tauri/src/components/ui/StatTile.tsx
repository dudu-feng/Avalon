// KPI 指标卡原子：标签 + 大数值 + 可选环比。
//
// 数值用 --font-sans（不用 --font-display：衬线体在大数字上像装饰），
// 且用比例数字 —— tabular-nums 会让每个数字都占「0」的宽度，
// 大号字下 121 这种数看起来会松垮。tabular-nums 只留给需要纵向对齐的表格列。
//
// 环比一律用中性色 + 箭头，不用红绿：token 用量的涨跌没有绝对好坏，
// 且 success/error 是保留的状态色，不能挪作他用。

import type { HTMLAttributes } from 'react';
import styles from './StatTile.module.css';

export type DeltaDirection = 'up' | 'down' | 'flat';

export interface StatTileProps extends HTMLAttributes<HTMLDivElement> {
  /** 指标名，句子式、不带尾冒号 */
  label: string;
  /** 已格式化的数值文本 */
  value: string;
  /** 已格式化的环比文本，如 "12.3%"；省略则不显示环比行 */
  delta?: string;
  /** 环比方向，决定箭头 */
  deltaDirection?: DeltaDirection;
  /** 环比的对比说明，默认「较上期」 */
  deltaLabel?: string;
  /** 数值下方的补充说明 */
  hint?: string;
}

const ARROW: Record<DeltaDirection, string> = {
  up: '↑',
  down: '↓',
  flat: '·',
};

export function StatTile({
  label,
  value,
  delta,
  deltaDirection = 'flat',
  deltaLabel = '较上期',
  hint,
  className = '',
  ...rest
}: StatTileProps) {
  return (
    <div className={[styles.tile, className].filter(Boolean).join(' ')} {...rest}>
      <p className={styles.label}>{label}</p>
      <p className={styles.value}>{value}</p>
      {delta ? (
        <p className={styles.delta}>
          <span className={styles.arrow} aria-hidden="true">
            {ARROW[deltaDirection]}
          </span>
          {delta}
          <span className={styles.deltaLabel}>{deltaLabel}</span>
        </p>
      ) : (
        hint && <p className={styles.hint}>{hint}</p>
      )}
    </div>
  );
}
