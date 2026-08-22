// 定时任务协议类型（对齐后端 scheduler 模块 serde 序列化，snake_case）
//
// 后端 ScheduleType 用 #[serde(tag = "type", rename_all = "snake_case")]，
// 所以 schedule 是带 type 判别字段的联合；TaskSource / RunStatus 用 snake_case。

/** 触发时间（判别联合，type 字段区分） */
export type ScheduleType =
  | { type: 'once'; at: string }
  | { type: 'daily'; time: string }
  | { type: 'weekly'; weekday: number; time: string };

/** 任务来源 */
export type TaskSource = 'user' | 'agent';

/** 执行状态 */
export type RunStatus = 'succeeded' | 'failed';

/** 每次执行的轻量元数据（完整消息走 session） */
export interface TaskRunMeta {
  triggered_at: string;
  status: RunStatus;
  read: boolean;
}

/** 定时任务定义 */
export interface ScheduledTask {
  /** 任务 id，同时作为会话 channel（task_ 前缀） */
  id: string;
  source: TaskSource;
  name: string;
  prompt: string;
  schedule: ScheduleType;
  enabled: boolean;
  created_at: string;
  last_run_at: string | null;
  runs: TaskRunMeta[];
}
