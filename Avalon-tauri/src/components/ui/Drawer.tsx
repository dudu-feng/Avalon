// 左侧抽屉（drawer）容器：从左侧滑入的面板，用于承载「会话历史」这类默认隐藏的侧边内容。
// 遮罩 + 面板均 Portal 挂载；支持入场/退场过渡、遮罩点击关闭、Escape 关闭、内置右上角 × 关闭。
// z-index 900：低于浮层（Tooltip/Dropdown/Menu 1000）与 Modal（1100），保证抽屉内的二级菜单与确认框正常浮在其上。

import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import styles from './Drawer.module.css';

export interface DrawerProps {
  open: boolean;
  onClose: () => void;
  /** 面板标题（如「会话」），与 actions 均不传则省略 header */
  title?: ReactNode;
  /** 标题右侧操作区（如「＋ 新建」），与内置 × 关闭按钮并排 */
  actions?: ReactNode;
  /** 面板宽度（px），默认 300 */
  width?: number;
  children: ReactNode;
}

const TRANSITION_MS = 260;

export function Drawer({ open, onClose, title, actions, width = 300, children }: DrawerProps) {
  // 两态驱动过渡：mounted 控制挂载，visible 控制滑入/滑出
  const [mounted, setMounted] = useState(open);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (open) {
      setMounted(true);
      // 等挂载渲染后再加 visible，触发滑入过渡
      const t = setTimeout(() => setVisible(true), 16);
      return () => clearTimeout(t);
    }
    setVisible(false);
    const timer = setTimeout(() => setMounted(false), TRANSITION_MS);
    return () => clearTimeout(timer);
  }, [open]);

  // Escape 关闭
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [open, onClose]);

  if (!mounted) return null;

  const panelStyle = { '--drawer-width': `${width}px` } as CSSProperties;

  return createPortal(
    <div className={styles.root}>
      <div
        className={[styles.overlay, visible ? styles.visible : ''].filter(Boolean).join(' ')}
        onClick={onClose}
        aria-hidden
      />
      <aside
        className={[styles.panel, visible ? styles.visible : ''].filter(Boolean).join(' ')}
        style={panelStyle}
        role="dialog"
        aria-modal="true"
      >
        {(title != null || actions != null) && (
          <header className={styles.header}>
            <span className={styles.title}>{title}</span>
            <div className={styles.actions}>
              {actions}
              <button type="button" className={styles.close} onClick={onClose} aria-label="关闭">
                ×
              </button>
            </div>
          </header>
        )}
        <div className={styles.body}>{children}</div>
      </aside>
    </div>,
    document.body,
  );
}
