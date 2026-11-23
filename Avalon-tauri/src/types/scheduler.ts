// 定时任务协议类型（对齐后端 scheduler 模块 serde 序列化，snake_case）
//
// 后端 ScheduleType 用 #[serde(tag = "type", rename_all = "snake_case")]，
// 所以 schedule 是带 type 判别字段的联合；TaskSource / RunStatus 用 snake_case。
// 任务不再拥有独立会话：执行结果写入统一主会话（source=task），通知渠道单独配置。

/** 触发时间（判别联合，type 字段区分） */
export type ScheduleType =
  | { type: 'once'; at: string }
  | { type: 'daily'; time: string }
  | { type: 'weekly'; weekday: number; time: string };

/** 任务来源 */
export type TaskSource = 'user' | 'agent';

/** 执行状态 */
export type RunStatus = 'succeeded' | 'failed';

/** 结果通知渠道（none = 只写入主会话，不主动发送） */
export type TaskNotifyChannel = 'none' | 'desktop' | 'feishu' | 'qq';

/** 每次执行的轻量元数据（完整对话在统一主会话时间线里） */
export interface TaskRunMeta {
  triggered_at: string;
  status: RunStatus;
  read: boolean;
  /** 通知投递失败原因（不影响任务本身的成功状态） */
  notify_error?: string;
}

/** 定时任务定义 */
export interface ScheduledTask {
  id: string;
  source: TaskSource;
  name: string;
  prompt: string;
  schedule: ScheduleType;
  enabled: boolean;
  created_at: string;
  last_run_at: string | null;
  runs: TaskRunMeta[];
  notify_channel: TaskNotifyChannel;
  /** 可选通知目标；缺省时发给主人 */
  notify_target?: string | null;
}
