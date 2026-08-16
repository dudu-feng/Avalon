import type { ReactNode } from 'react';
import styles from './MainLayout.module.css';

export interface MainLayoutProps {
  sidebar: ReactNode;
  header: ReactNode;
  children: ReactNode;
}

export function MainLayout({ sidebar, header, children }: MainLayoutProps) {
  return (
    <div className={styles.layout}>
      {sidebar}
      <div className={styles.main}>
        {header}
        <main className={styles.content}>{children}</main>
      </div>
    </div>
  );
}
