import type { ReactNode } from 'react';
import { ScrollArea } from '../ui';
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
        <ScrollArea as="main" className={styles.contentRoot} viewportClassName={styles.content}>
          {children}
        </ScrollArea>
      </div>
    </div>
  );
}
