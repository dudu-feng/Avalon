// 悬浮提示（tooltip）原子组件：包裹触发元素，hover/focus 时在其周边弹出简短说明。
//
// Portal 挂载到 body + fixed 定位，规避滚动容器 overflow 裁剪与侧边栏遮挡；
// 延迟出现（delayMs）+ 淡入缩放动画，消失即时；打开期间滚动/缩放即关闭，避免位置过期。

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { CSSProperties, ReactNode } from 'react';
import styles from './Tooltip.module.css';

export type TooltipSide = 'top' | 'bottom' | 'left' | 'right';

export interface TooltipProps {
  /** 提示内容（简短说明，支持 ReactNode） */
  label: ReactNode;
  /** 出现方向，默认 top */
  side?: TooltipSide;
  /** 出现延迟 ms，默认 300 */
  delayMs?: number;
  /** 触发元素 */
  children: ReactNode;
}

const GAP = 8; // 气泡与触发元素的间距
const PAD = 8; // 视口边缘安全间距

export function Tooltip({ label, side = 'top', delayMs = 300, children }: TooltipProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<CSSProperties>({});
  const triggerRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLSpanElement>(null);
  const timerRef = useRef<number | undefined>(undefined);

  const show = () => {
    window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => setOpen(true), delayMs);
  };

  const hide = () => {
    window.clearTimeout(timerRef.current);
    setOpen(false);
  };

  // 打开后测量触发元素与气泡尺寸，计算 fixed 坐标（水平/垂直钳制在视口内）
  useLayoutEffect(() => {
    if (!open) return;
    const trigger = triggerRef.current;
    const tip = tipRef.current;
    if (!trigger || !tip) return;

    const tr = trigger.getBoundingClientRect();
    const tipRect = tip.getBoundingClientRect();
    let top = 0;
    let left = 0;

    switch (side) {
      case 'top':
        top = tr.top - tipRect.height - GAP;
        left = tr.left + tr.width / 2 - tipRect.width / 2;
        break;
      case 'bottom':
        top = tr.bottom + GAP;
        left = tr.left + tr.width / 2 - tipRect.width / 2;
        break;
      case 'left':
        top = tr.top + tr.height / 2 - tipRect.height / 2;
        left = tr.left - tipRect.width - GAP;
        break;
      case 'right':
        top = tr.top + tr.height / 2 - tipRect.height / 2;
        left = tr.right + GAP;
        break;
    }

    // 兜底钳制：极端靠边时不溢出屏幕（箭头可能略偏离触发元素，属可接受取舍）
    left = Math.min(Math.max(left, PAD), window.innerWidth - tipRect.width - PAD);
    top = Math.min(Math.max(top, PAD), window.innerHeight - tipRect.height - PAD);

    setPos({ top, left });
  }, [open, side]);

  // 打开期间滚动/缩放即关闭，避免位置漂移
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  }, [open]);

  // 卸载时清理延迟定时器
  useEffect(() => () => window.clearTimeout(timerRef.current), []);

  return (
    <>
      <div
        ref={triggerRef}
        className={styles.root}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        {children}
      </div>
      {createPortal(
        <span
          ref={tipRef}
          className={styles.tip}
          data-side={side}
          data-open={open}
          style={pos}
          role="tooltip"
        >
          {label}
        </span>,
        document.body,
      )}
    </>
  );
}
