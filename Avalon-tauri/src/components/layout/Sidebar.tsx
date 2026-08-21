import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { MenuItem } from './MenuItem';
import { ScrollArea } from '../ui';
import styles from './Sidebar.module.css';
import type { MenuItemData } from '../../types';
import blackLogo from '../../assets/avalon-logo/Avalon-black.png';
import creamLogo from '../../assets/avalon-logo/Avalon-cream.png';

/** 关于入口在底部区用版本号替代 label 显示 */
const ABOUT_ID = 'about';

export interface SidebarProps {
  title: string;
  items: MenuItemData[];
  activeId: string;
  onSelect: (id: string) => void;
}

export function Sidebar({ title, items, activeId, onSelect }: SidebarProps) {
  // 版本号从 tauri.conf.json 动态读取，format 成 major.minor（v0.1）
  const [version, setVersion] = useState('v0.1');

  useEffect(() => {
    getVersion()
      .then((v) => setVersion('v' + v.split('.').slice(0, 2).join('.')))
      .catch(() => {});
  }, []);

  const navItems = items.filter((item) => item.position !== 'bottom');
  const bottomItems = items.filter((item) => item.position === 'bottom');

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
        {navItems.map((item) => (
          <MenuItem
            key={item.id}
            label={item.label}
            icon={item.icon}
            active={activeId === item.id}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </nav>
      {bottomItems.length > 0 && (
        <div className={styles.footer}>
          {bottomItems.map((item) => (
            <MenuItem
              key={item.id}
              label={item.id === ABOUT_ID ? version : item.label}
              icon={item.icon}
              active={activeId === item.id}
              onClick={() => onSelect(item.id)}
            />
          ))}
        </div>
      )}
    </ScrollArea>
  );
}
