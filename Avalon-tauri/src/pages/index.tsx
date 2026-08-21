import type { ComponentType } from 'react';
import type { MenuItemData } from '../types';
import { HomeIcon, ChatIcon, LibraryIcon, SettingsIcon, InfoIcon } from '../components/icons';
import { HomePage } from './HomePage';
import { ChatPage } from './ChatPage';
import { LibraryPage } from './LibraryPage';
import { SettingsPage } from './SettingsPage';
import { AboutPage } from './AboutPage';

export interface PageConfig extends MenuItemData {
  component: ComponentType;
}

export const pages: PageConfig[] = [
  { id: 'home', label: 'Home', icon: <HomeIcon />, component: HomePage },
  { id: 'chat', label: 'Chat', icon: <ChatIcon />, component: ChatPage },
  { id: 'library', label: 'Library', icon: <LibraryIcon />, component: LibraryPage },
  { id: 'settings', label: 'Settings', icon: <SettingsIcon />, component: SettingsPage, position: 'bottom' },
  { id: 'about', label: 'About', icon: <InfoIcon />, component: AboutPage, position: 'bottom' },
];
