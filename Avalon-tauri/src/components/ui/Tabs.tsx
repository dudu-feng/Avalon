// 标签页（tabs）原子组件：底部一条滑动下划线指示器标记当前激活项。
//
// 指示器用 useLayoutEffect 在 DOM 提交后、绘制前测量激活 tab 的 offsetLeft/offsetWidth，
// 再通过 transform 平移 + width 过渡实现「滑块滑动」的连续动画（切 tab 时平滑位移）。

import { useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import styles from './Tabs.module.css';

export interface TabsOption {
  value: string;
  label: ReactNode;
}

export interface TabsProps {
  options: TabsOption[];
  value: string;
  onChange: (value: string) => void;
}

export function Tabs({ options, value, onChange }: TabsProps) {
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [indicator, setIndicator] = useState<CSSProperties>({
    width: 0,
    transform: 'translateX(0px)',
  });

  useLayoutEffect(() => {
    const idx = options.findIndex((o) => o.value === value);
    const el = tabRefs.current[idx];
    if (!el) return;
    setIndicator({
      width: el.offsetWidth,
      transform: `translateX(${el.offsetLeft}px)`,
    });
  }, [value, options]);

  return (
    <div className={styles.tabs} role="tablist">
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => {
            tabRefs.current[i] = el;
          }}
          type="button"
          role="tab"
          aria-selected={o.value === value}
          className={[styles.tab, o.value === value ? styles.active : '']
            .filter(Boolean)
            .join(' ')}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
      <span className={styles.indicator} style={indicator} aria-hidden="true" />
    </div>
  );
}
