// 下拉选择（dropdown）原子组件：按钮触发弹出菜单，自定义外观替代原生 select。
// Portal 挂载 + fixed 定位（同 Tooltip），规避滚动容器裁剪；支持键盘导航与 ARIA 无障碍。

import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react';
import styles from './Dropdown.module.css';

export interface DropdownOption {
  value: string;
  label: ReactNode;
}

export interface DropdownProps {
  options: DropdownOption[];
  value: string;
  onChange: (value: string) => void;
  /** 触发按钮自定义内容，不传则显示当前选中项 label */
  trigger?: ReactNode;
  /** 菜单水平对齐，默认 start（左对齐按钮） */
  align?: 'start' | 'end';
  /** 无匹配项时触发按钮的占位文字 */
  placeholder?: string;
  /** 可选字段标签（表单场景），渲染在触发按钮上方 */
  label?: ReactNode;
  disabled?: boolean;
  className?: string;
}

const MENU_GAP = 6;
const PAD = 8;

export function Dropdown({
  options,
  value,
  onChange,
  trigger,
  align = 'start',
  placeholder = '请选择',
  label,
  disabled = false,
  className = '',
}: DropdownProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<CSSProperties>({});
  const [activeIndex, setActiveIndex] = useState(-1);
  const btnRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const uid = useId();

  const current = options.find((o) => o.value === value);

  const close = useCallback(() => {
    setOpen(false);
    setActiveIndex(-1);
  }, []);

  const toggle = () => {
    if (disabled) return;
    if (open) close();
    else setOpen(true);
  };

  const select = (v: string) => {
    onChange(v);
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
  }, [open, align, options]);

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

  // 打开时把高亮重置为当前选中项，键盘导航从它开始
  useEffect(() => {
    if (open) setActiveIndex(options.findIndex((o) => o.value === value));
  }, [open, options, value]);

  // 高亮项滚入视野
  useEffect(() => {
    if (!open || activeIndex < 0 || !menuRef.current) return;
    const item = menuRef.current.children[activeIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: 'nearest' });
  }, [open, activeIndex]);

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
        setActiveIndex((i) => (i + 1) % options.length);
        break;
      case 'ArrowUp':
        e.preventDefault();
        setActiveIndex((i) => (i - 1 + options.length) % options.length);
        break;
      case 'Home':
        e.preventDefault();
        setActiveIndex(0);
        break;
      case 'End':
        e.preventDefault();
        setActiveIndex(options.length - 1);
        break;
      case 'Enter':
      case ' ':
        e.preventDefault();
        if (activeIndex >= 0 && options[activeIndex]) select(options[activeIndex].value);
        break;
      case 'Tab':
        close();
        break;
    }
  };

  const optId = (i: number) => `${uid}-opt-${i}`;

  const triggerButton = (
    <button
      ref={btnRef}
      type="button"
      id={uid}
      className={[styles.trigger, className].filter(Boolean).join(' ')}
      data-open={open}
      aria-haspopup="listbox"
      aria-expanded={open}
      disabled={disabled}
      onClick={toggle}
      onKeyDown={onTriggerKeyDown}
    >
      <span className={styles.triggerLabel}>{trigger ?? current?.label ?? placeholder}</span>
    </button>
  );

  return (
    <>
      {label != null ? (
        <div className={styles.wrapper}>
          <label className={styles.label} htmlFor={uid}>
            {label}
          </label>
          {triggerButton}
        </div>
      ) : (
        triggerButton
      )}
      {open &&
        createPortal(
          <div
            ref={menuRef}
            className={styles.menu}
            style={pos}
            role="listbox"
            aria-activedescendant={activeIndex >= 0 ? optId(activeIndex) : undefined}
          >
            {options.map((opt, i) => (
              <div
                key={opt.value}
                id={optId(i)}
                className={[
                  styles.option,
                  opt.value === value ? styles.selected : '',
                  i === activeIndex ? styles.active : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                role="option"
                aria-selected={opt.value === value}
                onClick={() => select(opt.value)}
                onMouseEnter={() => setActiveIndex(i)}
              >
                <span className={styles.optionLabel}>{opt.label}</span>
              </div>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}
