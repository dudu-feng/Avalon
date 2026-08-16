import { MenuItem } from './MenuItem';
import styles from './Sidebar.module.css';
import type { MenuItemData } from '../../types';

export interface SidebarProps {
  title: string;
  items: MenuItemData[];
  activeId: string;
  onSelect: (id: string) => void;
}

export function Sidebar({ title, items, activeId, onSelect }: SidebarProps) {
  return (
    <aside className={styles.sidebar} aria-label="Sidebar navigation">
      <p className={styles.brand}>{title}</p>
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
    </aside>
  );
}
