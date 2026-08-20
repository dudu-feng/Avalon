// 圆形进度条原子组件：SVG 双圆环（背景环 + 前景环）+ 中心文字。
// 通用进度呈现，本次用于「上下文用量」：value = max_input_tokens，max = compress_threshold。

import type { HTMLAttributes, ReactNode } from 'react';
import styles from './CircleProgress.module.css';

export interface CircleProgressProps extends HTMLAttributes<HTMLDivElement> {
  /** 当前值（与 max 同单位；上下文用量传 input_tokens） */
  value: number;
  /** 满值（默认 100；上下文用量传 compress_threshold） */
  max?: number;
  /** 直径 px（默认 44） */
  size?: number;
  /** 环厚 px（默认 4） */
  strokeWidth?: number;
  /** 中心内容，默认显示百分比文本 */
  label?: ReactNode;
}

export function CircleProgress({
  value,
  max = 100,
  size = 44,
  strokeWidth = 4,
  label,
  className = '',
  ...rest
}: CircleProgressProps) {
  const fraction = max > 0 ? Math.min(Math.max(value / max, 0), 1) : 0;
  const radius = (size - strokeWidth) / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference * (1 - fraction);
  const center = size / 2;

  return (
    <div
      className={[styles.root, className].filter(Boolean).join(' ')}
      style={{ width: size, height: size }}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      {...rest}
    >
      <svg width={size} height={size} className={styles.svg}>
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke="var(--muted)"
          strokeWidth={strokeWidth}
        />
        <circle
          cx={center}
          cy={center}
          r={radius}
          fill="none"
          stroke="var(--primary)"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={offset}
          transform={`rotate(-90 ${center} ${center})`}
          className={styles.fill}
        />
      </svg>
      <span className={styles.label} style={{ fontSize: Math.round(size * 0.26) }}>
        {label ?? `${Math.round(fraction * 100)}%`}
      </span>
    </div>
  );
}
