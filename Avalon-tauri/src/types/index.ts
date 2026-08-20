export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md';

export type CardVariant = 'default' | 'sunken' | 'emphasis';

export type BadgeVariant = 'filled' | 'muted' | 'outline';

export interface MenuItemData {
  id: string;
  label: string;
  icon?: React.ReactNode;
}

export type ThemeMode = 'light' | 'dark' | 'system';
