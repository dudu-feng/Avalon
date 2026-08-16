import { useEffect, useState } from 'react';
import type { ThemeMode } from '../types';

const STORAGE_KEY = 'avalon-theme';

function resolveTheme(mode: ThemeMode): 'light' | 'dark' {
  if (mode !== 'system') return mode;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function useTheme() {
  const [mode, setMode] = useState<ThemeMode>(() => {
    const stored = localStorage.getItem(STORAGE_KEY) as ThemeMode | null;
    return stored ?? 'system';
  });

  useEffect(() => {
    const root = document.documentElement;
    const apply = () => {
      const resolved = resolveTheme(mode);
      root.classList.toggle('dark', resolved === 'dark');
    };

    apply();
    localStorage.setItem(STORAGE_KEY, mode);

    if (mode === 'system') {
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      const handler = () => apply();
      mq.addEventListener('change', handler);
      return () => mq.removeEventListener('change', handler);
    }
  }, [mode]);

  return { mode, setMode };
}
