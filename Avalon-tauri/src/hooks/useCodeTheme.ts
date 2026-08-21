// 代码块配色主题：三态（经典白 / 深色 / 奶油），全局共享 + 本地持久化。
//
// 与 useTheme（应用主题）相互独立：应用主题只管 html.dark，
// 代码块配色由用户在代码块右上角按钮单独切换。
// 用 useSyncExternalStore 做成模块级单例 store——所有 MarkdownContent 实例
// 读同一个值、任一处切换即全局同步，无需在组件树挂 Provider。

import { useSyncExternalStore } from 'react';
import type { ThemeMode } from '../types';

const STORAGE_KEY = 'avalon-code-theme';

/** 循环顺序：经典白 → 深色 → 奶油 → 经典白 */
const ORDER: ThemeMode[] = ['light', 'dark', 'system'];

function readStored(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system';
}

let current: ThemeMode = readStored();
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): ThemeMode {
  return current;
}

export function useCodeTheme() {
  const codeTheme = useSyncExternalStore(subscribe, getSnapshot);

  const cycle = () => {
    current = ORDER[(ORDER.indexOf(current) + 1) % ORDER.length];
    localStorage.setItem(STORAGE_KEY, current);
    emit();
  };

  return { codeTheme, cycle };
}
