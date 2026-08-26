import type { ComponentType } from 'react';
import type { MenuItemData } from '../types';
import {
  DashboardIcon,
  ChatIcon,
  ClockIcon,
  SettingsIcon,
  InfoIcon,
} from '../components/icons';
import { DashboardPage } from './DashboardPage';
import { ChatPage } from './ChatPage';
import { SchedulePage } from './SchedulePage';
import { SettingsPage } from './SettingsPage';
import { AboutPage } from './AboutPage';

export interface PageConfig extends MenuItemData {
  component: ComponentType;
}

export const pages: PageConfig[] = [
  { id: 'dashboard', label: '仪表盘', icon: <DashboardIcon />, component: DashboardPage },
  { id: 'chat', label: '对话', icon: <ChatIcon />, component: ChatPage },
  { id: 'schedule', label: '定时任务', icon: <ClockIcon />, component: SchedulePage },
  { id: 'settings', label: '设置', icon: <SettingsIcon />, component: SettingsPage, position: 'bottom' },
  { id: 'about', label: '关于', icon: <InfoIcon />, component: AboutPage, position: 'bottom' },
];
