// 骨架屏（skeleton）原子组件：shimmer 高光扫过，用于加载占位，避免内容跳动/静止。
// 尺寸、形状、对齐由使用方通过 className / style 控制，本组件只负责底色与扫光动画。

import type { HTMLAttributes } from 'react';
import styles from './Skeleton.module.css';

export interface SkeletonProps extends HTMLAttributes<HTMLDivElement> {}

export function Skeleton({ className = '', ...rest }: SkeletonProps) {
  return <div className={[styles.skeleton, className].filter(Boolean).join(' ')} {...rest} />;
}
