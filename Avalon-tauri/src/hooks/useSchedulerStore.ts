// 定时任务状态 store：任务列表 + 未读角标，模块级单例（useSyncExternalStore）。
//
// 监听全局事件 task-finished，任务完成后自动刷新列表与角标。
// initScheduler 幂等（initialized 标志），由 useScheduler 首次挂载时触发一次。

import { useEffect, useSyncExternalStore } from 'react';
import type { ScheduledTask } from '../types/scheduler';
import {
  createScheduledTask,
  deleteScheduledTask,
  getUnreadTaskCount,
  listScheduledTasks,
  markTaskRead,
  onTaskFinished,
  toggleScheduledTask,
  type ScheduleKind,
} from '../lib/schedulerApi';

interface SchedulerState {
  tasks: ScheduledTask[];
  unread: number;
  loaded: boolean;
}

let state: SchedulerState = { tasks: [], unread: 0, loaded: false };
const listeners = new Set<() => void>();

function emit() {
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): SchedulerState {
  return state;
}

/** 拉取任务列表 + 未读数，替换 state 触发订阅者刷新（内部吞错，不向上抛） */
export async function refreshScheduler() {
  try {
    const [tasks, unread] = await Promise.all([listScheduledTasks(), getUnreadTaskCount()]);
    state = { tasks, unread, loaded: true };
    emit();
  } catch (e) {
    console.error('刷新定时任务失败:', e);
  }
}

let initialized = false;

/** 应用级初始化：注册任务完成事件订阅 + 首次拉取（幂等） */
export function initScheduler() {
  if (initialized) return;
  initialized = true;
  onTaskFinished(() => {
    refreshScheduler();
  });
  refreshScheduler();
}

/** 组件用：读状态 + 操作（操作后自动刷新） */
export function useScheduler() {
  const s = useSyncExternalStore(subscribe, getSnapshot);

  useEffect(() => {
    initScheduler();
  }, []);

  const create = async (
    name: string,
    prompt: string,
    scheduleType: ScheduleKind,
    scheduleValue: string,
  ) => {
    await createScheduledTask(name, prompt, scheduleType, scheduleValue);
    await refreshScheduler();
  };
  const remove = async (id: string) => {
    await deleteScheduledTask(id);
    await refreshScheduler();
  };
  const toggle = async (id: string, enabled: boolean) => {
    await toggleScheduledTask(id, enabled);
    await refreshScheduler();
  };
  const markRead = async (id: string) => {
    await markTaskRead(id);
    await refreshScheduler();
  };

  return { ...s, create, remove, toggle, markRead };
}
