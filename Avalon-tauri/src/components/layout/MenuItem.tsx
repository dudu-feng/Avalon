import type { ButtonHTMLAttributes, ReactNode } from 'react';
import styles from './MenuItem.module.css';

export interface MenuItemProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  icon?: ReactNode;
  active?: boolean;
  /** 右侧角标计数（如未读数），> 0 才显示 */
  badge?: number;
}

export function MenuItem({
  label,
  icon,
  active = false,
  badge,
  className = '',
  ...rest
}: MenuItemProps) {
  const classes = [styles.item, active && styles.active, className]
    .filter(Boolean)
    .join(' ');

  return (
    <button type="button" className={classes} {...rest}>
      {icon && <span className={styles.icon}>{icon}</span>}
      <span className={styles.label}>{label}</span>
      {badge != null && badge > 0 && (
        <span className={styles.badge}>{badge > 99 ? '99+' : badge}</span>
      )}
    </button>
  );
}
