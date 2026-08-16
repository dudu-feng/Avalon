import { Button } from './Button';
import type { ThemeMode } from '../../types';

const labels: Record<ThemeMode, string> = {
  light: 'Light',
  dark: 'Dark',
  system: 'System',
};

const order: ThemeMode[] = ['light', 'dark', 'system'];

export interface ThemeToggleProps {
  mode: ThemeMode;
  onChange: (mode: ThemeMode) => void;
}

export function ThemeToggle({ mode, onChange }: ThemeToggleProps) {
  const handleClick = () => {
    const nextIndex = (order.indexOf(mode) + 1) % order.length;
    onChange(order[nextIndex]);
  };

  return (
    <Button variant="ghost" size="sm" onClick={handleClick}>
      {labels[mode]}
    </Button>
  );
}
