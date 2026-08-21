export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md';

export type CardVariant = 'default' | 'sunken' | 'emphasis';

export type BadgeVariant = 'filled' | 'muted' | 'outline';

export type MenuPosition = 'top' | 'bottom';

export interface MenuItemData {
  id: string;
  label: string;
  icon?: React.ReactNode;
  /** 菜单位置：top = 导航区（默认），bottom = 侧边栏底部（设置 / 关于等系统入口） */
  position?: MenuPosition;
}

export type ThemeMode = 'light' | 'dark' | 'system';
