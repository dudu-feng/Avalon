import type { HTMLAttributes, ReactNode } from 'react';
import type { CardVariant } from '../../types';
import styles from './Card.module.css';

export interface CardProps extends HTMLAttributes<HTMLElement> {
  as?: 'article' | 'div' | 'section';
  variant?: CardVariant;
  eyebrow?: string;
  title?: string;
  description?: string;
  children?: ReactNode;
  disabled?: boolean;
}

export function Card({
  as: Component = 'article',
  variant = 'default',
  eyebrow,
  title,
  description,
  children,
  disabled = false,
  className = '',
  ...rest
}: CardProps) {
  const classes = [styles.card, styles[variant], className].filter(Boolean).join(' ');

  return (
    <Component
      className={classes}
      aria-disabled={disabled || undefined}
      {...rest}
    >
      {eyebrow && <p className={styles.eyebrow}>{eyebrow}</p>}
      {title && <h3 className={styles.title}>{title}</h3>}
      {description && <p className={styles.body}>{description}</p>}
      {children}
    </Component>
  );
}
