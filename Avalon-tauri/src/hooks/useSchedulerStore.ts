// 定时任务状态 store：任务列表 + 未读角标，模块级单例（useSyncExternalStore）。
//
// 监听全局事件 task-finished，任务完成后自动刷新列表与角标。
// initScheduler 幂等（initialized 标志），由 useScheduler 首次挂载时触发一次。

import { useEffect, useSyncExternalStore } from 'react';
import type { ScheduledTask, TaskNotifyChannel } from '../types/scheduler';
import {
  createScheduledTask,
  deleteScheduledTask,
  getUnreadTaskCount,
  listScheduledTasks,
  markTaskRead,
  onTaskFinished,
  onTaskNotification,
  toggleScheduledTask,
  updateScheduledTask,
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

/** 应用级初始化：注册任务完成事件订阅（幂等，全局只订阅一次） */
export function initScheduler() {
  if (initialized) return;
  initialized = true;
  // 订阅是异步的：失败时复位标志，下次挂载还能重试；同时吞掉 rejection
  onTaskFinished(() => {
    void refreshScheduler();
  }).catch((e) => {
    initialized = false;
    console.error('订阅 task-finished 失败:', e);
  });
  // 任务 notify_channel = desktop 时后端广播；WebView 的 Notification 会落到系统通知中心。
  // 权限未授予时先请求一次；失败只记日志，不影响任务本身。
  onTaskNotification(({ title, body }) => {
    try {
      if (typeof Notification === 'undefined') return;
      if (Notification.permission === 'granted') {
        new Notification(title, { body });
      } else if (Notification.permission !== 'denied') {
        void Notification.requestPermission().then((p) => {
          if (p === 'granted') new Notification(title, { body });
        });
      }
    } catch (e) {
      console.error('显示任务通知失败:', e);
    }
  }).catch((e) => console.error('订阅 task-notification 失败:', e));
}

/** 组件用：读状态 + 操作（操作后自动刷新） */
export function useScheduler() {
  const s = useSyncExternalStore(subscribe, getSnapshot);

  useEffect(() => {
    initScheduler();
    // 每次挂载都刷新：页面切走再切回时重新拉取，避免拿到启动时的旧快照
    refreshScheduler();
  }, []);

  const create = async (
    name: string,
    prompt: string,
    scheduleType: ScheduleKind,
    scheduleValue: string,
    notifyChannel: TaskNotifyChannel = 'none',
    notifyTarget: string | null = null,
  ) => {
    await createScheduledTask(
      name,
      prompt,
      scheduleType,
      scheduleValue,
      notifyChannel,
      notifyTarget,
    );
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
  const update = async (
    id: string,
    name: string,
    prompt: string,
    scheduleType: ScheduleKind,
    scheduleValue: string,
    notifyChannel: TaskNotifyChannel = 'none',
    notifyTarget: string | null = null,
  ) => {
    await updateScheduledTask(
      id,
      name,
      prompt,
      scheduleType,
      scheduleValue,
      notifyChannel,
      notifyTarget,
    );
    await refreshScheduler();
  };
  const markRead = async (id: string) => {
    await markTaskRead(id);
    await refreshScheduler();
  };

  return { ...s, create, remove, toggle, update, markRead };
}
