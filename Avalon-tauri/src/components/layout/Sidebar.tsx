import { MenuItem } from './MenuItem';
import { ScrollArea } from '../ui';
import styles from './Sidebar.module.css';
import type { MenuItemData } from '../../types';
import blackLogo from '../../assets/avalon-logo/Avalon-black.png';
import creamLogo from '../../assets/avalon-logo/Avalon-cream.png';

export interface SidebarProps {
  title: string;
  items: MenuItemData[];
  activeId: string;
  onSelect: (id: string) => void;
}

export function Sidebar({ title, items, activeId, onSelect }: SidebarProps) {
  return (
    <ScrollArea
      as="aside"
      className={styles.sidebar}
      viewportClassName={styles.sidebarViewport}
      aria-label="Sidebar navigation"
    >
      <div className={styles.brand}>
        <img className={styles.logo} src={creamLogo} alt="" />
        <img className={styles.logoDark} src={blackLogo} alt="" />
        <span className={styles.brandText}>{title}</span>
      </div>
      <nav className={styles.stack}>
        {items.map((item) => (
          <MenuItem
            key={item.id}
            label={item.label}
            icon={item.icon}
            active={activeId === item.id}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </nav>
    </ScrollArea>
  );
}
