// 操作菜单（menu）原子组件：触发按钮弹出动作菜单，用于「…」更多操作这类二级展示。
// Portal 挂载 + fixed 定位（同 Dropdown/Tooltip），规避滚动容器裁剪；支持键盘导航与 ARIA 无障碍。
// 与 Dropdown 的差异：动作语义——点击菜单项即执行 onSelect 并关闭，无 value / 选中态 / ✓ 标记。

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react';
import styles from './Menu.module.css';

export interface MenuItem {
  label: ReactNode;
  /** 破坏性操作（如删除），红字提示 */
  danger?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

export interface MenuProps {
  /** 触发按钮内容（如「…」），按钮由 Menu 内部渲染以便持有定位 ref */
  trigger: ReactNode;
  items: MenuItem[];
  /** 菜单水平对齐，默认 start（左对齐按钮） */
  align?: 'start' | 'end';
  /** 触发按钮无障碍标签 */
  ariaLabel?: string;
  /** 透传触发按钮样式（定制尺寸/颜色，覆盖默认极简样式） */
  className?: string;
  disabled?: boolean;
}

const MENU_GAP = 6;
const PAD = 8;

export function Menu({
  trigger,
  items,
  align = 'start',
  ariaLabel,
  className = '',
  disabled = false,
}: MenuProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<CSSProperties>({});
  const [activeIndex, setActiveIndex] = useState(-1);
  const btnRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const close = useCallback(() => {
    setOpen(false);
    setActiveIndex(-1);
  }, []);

  const toggle = () => {
    if (disabled) return;
    if (open) close();
    else setOpen(true);
  };

  const select = (item: MenuItem) => {
    if (item.disabled) return;
    item.onSelect();
    close();
  };

  // 定位：菜单在按钮下方，下方空间不足则翻上；按 align 对齐并水平钳制视口
  useLayoutEffect(() => {
    if (!open) return;
    const btn = btnRef.current;
    const menu = menuRef.current;
    if (!btn || !menu) return;
    const br = btn.getBoundingClientRect();
    const mr = menu.getBoundingClientRect();
    let top = br.bottom + MENU_GAP;
    if (top + mr.height > window.innerHeight - PAD) {
      top = br.top - mr.height - MENU_GAP;
    }
    let left = align === 'end' ? br.right - mr.width : br.left;
    left = Math.min(Math.max(left, PAD), window.innerWidth - mr.width - PAD);
    setPos({ top, left });
  }, [open, align]);

  // 打开时：点击外部 / Escape 关闭
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (btnRef.current?.contains(t) || menuRef.current?.contains(t)) return;
      close();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open, close]);

  // 打开时重置高亮（动作菜单无选中项），键盘导航从首项开始
  useEffect(() => {
    if (open) setActiveIndex(-1);
  }, [open]);

  // 高亮项滚入视野
  useEffect(() => {
    if (!open || activeIndex < 0 || !menuRef.current) return;
    const item = menuRef.current.children[activeIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: 'nearest' });
  }, [open, activeIndex]);

  // 键盘导航到下一个可用（非 disabled）项；无则返回 -1
  const nextEnabled = (from: number, dir: 1 | -1): number => {
    const n = items.length;
    if (n === 0) return -1;
    for (let step = 1; step <= n; step++) {
      const idx = (((from + dir * step) % n) + n) % n;
      if (!items[idx].disabled) return idx;
    }
    return -1;
  };

  const onTriggerKeyDown = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        setOpen(true);
      }
      return;
    }
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setActiveIndex((i) => nextEnabled(i, 1));
        break;
      case 'ArrowUp':
        e.preventDefault();
        setActiveIndex((i) => nextEnabled(i, -1));
        break;
      case 'Home':
        e.preventDefault();
        setActiveIndex(nextEnabled(-1, 1));
        break;
      case 'End':
        e.preventDefault();
        setActiveIndex(nextEnabled(items.length, -1));
        break;
      case 'Enter':
      case ' ':
        e.preventDefault();
        if (activeIndex >= 0 && items[activeIndex] && !items[activeIndex].disabled) {
          select(items[activeIndex]);
        }
        break;
      case 'Tab':
        close();
        break;
    }
  };

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className={[styles.trigger, className].filter(Boolean).join(' ')}
        aria-label={ariaLabel}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={toggle}
        onKeyDown={onTriggerKeyDown}
      >
        {trigger}
      </button>
      {open &&
        createPortal(
          <div ref={menuRef} className={styles.menu} style={pos} role="menu">
            {items.map((item, i) => (
              <div
                key={i}
                className={[
                  styles.item,
                  i === activeIndex ? styles.active : '',
                  item.danger ? styles.danger : '',
                  item.disabled ? styles.disabled : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                role="menuitem"
                aria-disabled={item.disabled || undefined}
                onClick={() => select(item)}
                onMouseEnter={() => {
                  if (!item.disabled) setActiveIndex(i);
                }}
              >
                {item.label}
              </div>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}
