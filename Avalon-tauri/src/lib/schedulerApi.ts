// 后端定时任务命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// 全局事件 task-finished 用 listen 订阅（后端 Scheduler 任务跑完 emit）。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ScheduledTask } from '../types/scheduler';

export type ScheduleKind = 'once' | 'daily' | 'weekly';

/** 创建定时任务（source=User），返回完整任务 */
export async function createScheduledTask(
  name: string,
  prompt: string,
  scheduleType: ScheduleKind,
  scheduleValue: string,
): Promise<ScheduledTask> {
  return invoke<ScheduledTask>('create_scheduled_task', {
    name,
    prompt,
    scheduleType,
    scheduleValue,
  });
}

/** 列出全部定时任务（创建时间倒序） */
export async function listScheduledTasks(): Promise<ScheduledTask[]> {
  return invoke<ScheduledTask[]>('list_scheduled_tasks');
}

/** 删除定时任务 */
export async function deleteScheduledTask(taskId: string): Promise<void> {
  await invoke('delete_scheduled_task', { taskId });
}

/** 停用 / 启用定时任务 */
export async function toggleScheduledTask(taskId: string, enabled: boolean): Promise<void> {
  await invoke('toggle_scheduled_task', { taskId, enabled });
}

/** 清除某任务未读标记（查看执行历史后调用） */
export async function markTaskRead(taskId: string): Promise<void> {
  await invoke('mark_task_read', { taskId });
}

/** 全部任务未读执行总数（驱动侧边栏角标） */
export async function getUnreadTaskCount(): Promise<number> {
  return invoke<number>('get_unread_task_count');
}

/** 订阅任务完成事件，返回取消订阅函数 */
export function onTaskFinished(cb: (taskId: string) => void): Promise<() => void> {
  return listen<string>('task-finished', (event) => cb(event.payload));
}
