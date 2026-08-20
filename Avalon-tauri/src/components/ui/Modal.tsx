// 模态框（modal）原子组件：Portal 挂载 + 居中面板，替代割裂的 window.confirm/alert。
//
// 支持遮罩点击 / Escape 关闭、打开时聚焦面板（或 [data-autofocus] 元素）并锁定背景滚动、
// 关闭后归还焦点、Tab 焦点陷阱，以及无障碍（role=dialog / aria-modal）。

import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import type { MouseEvent as ReactMouseEvent, ReactNode } from 'react';
import styles from './Modal.module.css';

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  /** 标题（可选） */
  title?: ReactNode;
  /** 说明文字（可选） */
  description?: ReactNode;
  /** 主体内容（可选，渲染在标题/说明下方、footer 上方） */
  children?: ReactNode;
  /** 底部操作区（可选，通常放按钮） */
  footer?: ReactNode;
  /** 面板最大宽度 px，默认 400 */
  width?: number;
}

const FOCUSABLE =
  'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 400,
}: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);
  // 用 ref 持有 onClose，避免其引用变化导致焦点 effect 反复重建
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  });

  useEffect(() => {
    if (!open) return;

    previouslyFocused.current = document.activeElement as HTMLElement | null;
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    // 优先聚焦带 data-autofocus 的元素（如确认框的安全按钮），否则聚焦面板本身
    const auto = panelRef.current?.querySelector<HTMLElement>('[data-autofocus]');
    (auto ?? panelRef.current)?.focus();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onCloseRef.current();
        return;
      }
      if (e.key !== 'Tab') return;
      const panel = panelRef.current;
      if (!panel) return;
      const focusables = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || active === panel)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = prevOverflow;
      previouslyFocused.current?.focus();
    };
  }, [open]);

  if (!open) return null;

  const onOverlayMouseDown = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) onClose();
  };

  return createPortal(
    <div className={styles.overlay} onMouseDown={onOverlayMouseDown}>
      <div
        ref={panelRef}
        className={styles.panel}
        style={{ maxWidth: width }}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
      >
        {title && <h2 className={styles.title}>{title}</h2>}
        {description && <p className={styles.description}>{description}</p>}
        {children && <div className={styles.body}>{children}</div>}
        {footer && <div className={styles.footer}>{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}
