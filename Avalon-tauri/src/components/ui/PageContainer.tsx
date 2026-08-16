import type { ReactNode } from 'react';
import styles from './PageContainer.module.css';

export interface PageContainerProps {
  title?: string;
  description?: string;
  children: ReactNode;
}

export function PageContainer({ title, description, children }: PageContainerProps) {
  return (
    <div className={styles.container}>
      {(title || description) && (
        <header className={styles.head}>
          {title && <h2 className={styles.title}>{title}</h2>}
          {description && <p className={styles.description}>{description}</p>}
        </header>
      )}
      {children}
    </div>
  );
}
