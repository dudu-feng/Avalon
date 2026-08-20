// 线性进度条原子组件：determinate（传 value）或 indeterminate（省略 value 的不确定扫动动画）。
// 本次用于「重建会话向量库」：首个进度事件到达前 indeterminate，之后 determinate 显示真实百分比。

import type { HTMLAttributes } from 'react';
import styles from './ProgressBar.module.css';

export interface ProgressBarProps extends HTMLAttributes<HTMLDivElement> {
  /** 进度值（0~max）；省略则 indeterminate 不确定动画 */
  value?: number;
  /** 满值（默认 100） */
  max?: number;
}

export function ProgressBar({ value, max = 100, className = '', ...rest }: ProgressBarProps) {
  const determinate = value !== undefined;
  const fraction = determinate ? Math.min(Math.max(value / max, 0), 1) : 0;

  return (
    <div
      className={[styles.root, className].filter(Boolean).join(' ')}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={determinate ? Math.round(value) : undefined}
      {...rest}
    >
      {determinate ? (
        <div className={styles.fill} style={{ width: `${fraction * 100}%` }} />
      ) : (
        <div className={`${styles.fill} ${styles.indeterminate}`} />
      )}
    </div>
  );
}
