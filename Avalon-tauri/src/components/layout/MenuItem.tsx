import type { ButtonHTMLAttributes, ReactNode } from 'react';
import styles from './MenuItem.module.css';

export interface MenuItemProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  icon?: ReactNode;
  active?: boolean;
}

export function MenuItem({
  label,
  icon,
  active = false,
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
    </button>
  );
}
