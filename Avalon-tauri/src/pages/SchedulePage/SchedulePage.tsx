// 定时任务页面：任务卡片列表 + 创建表单（Modal）+ 执行历史（Modal）。
//
// 任务不再是独立会话：每次触发以 source=task 的 user 消息写入统一主会话，
// 「历史」从主会话中按 origin.task_id 筛出该任务的轮次回放；
// 新执行经全局事件 task-finished 刷新未读角标，结果按 notify_channel 主动通知。

import { useEffect, useState } from 'react';
import { PageContainer, Badge, Button, Input, Modal, ConfirmDialog } from '../../components/ui';
import { MessageList } from '../../components/features/chat';
import { loadTaskMessages } from '../../components/features/chat/useChatStore';
import { useScheduler } from '../../hooks/useSchedulerStore';
import type { ScheduleKind } from '../../lib/schedulerApi';
import type { ChatMessage } from '../../types/chat';
import type {
  ScheduledTask,
  ScheduleType,
  TaskNotifyChannel,
} from '../../types/scheduler';
import styles from './SchedulePage.module.css';

const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'];

/** 新建/编辑表单的提交契约（通知目标空串在提交前归一为 null） */
type TaskFormSubmit = (
  name: string,
  prompt: string,
  scheduleType: ScheduleKind,
  value: string,
  notifyChannel: TaskNotifyChannel,
  notifyTarget: string | null,
) => Promise<void>;

/** 触发方式 → 人类可读 */
function humanSchedule(s: ScheduleType): string {
  switch (s.type) {
    case 'once':
      return `一次性 · ${s.at}`;
    case 'daily':
      return `每天 · ${s.time}`;
    case 'weekly':
      return `每周${WEEKDAYS[s.weekday - 1] ?? s.weekday} · ${s.time}`;
  }
}

/** 任务未读执行数 */
function unreadOf(task: ScheduledTask): number {
  return task.runs.filter((r) => !r.read).length;
}

/** 上次执行摘要 */
function lastRunText(task: ScheduledTask): string {
  if (task.runs.length === 0) return '尚未执行';
  const last = task.runs[task.runs.length - 1];
  const base = `上次执行 ${last.triggered_at} · ${last.status === 'succeeded' ? '成功' : '失败'}`;
  return last.notify_error ? `${base} · 通知失败` : base;
}

export function SchedulePage() {
  const { tasks, unread, loaded, create, remove, toggle, update, markRead } = useScheduler();

  // 表单：creating=新建，editing=编辑某任务（二者互斥）
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<ScheduledTask | null>(null);

  // 删除确认
  const [confirmDelete, setConfirmDelete] = useState<ScheduledTask | null>(null);

  // 执行历史（内部 TaskHistoryModal 用 useChat 驱动，可继续对话）
  const [viewing, setViewing] = useState<ScheduledTask | null>(null);

  const openCreate = () => {
    setEditing(null);
    setCreating(true);
  };

  const openEdit = (task: ScheduledTask) => {
    setCreating(false);
    setEditing(task);
  };

  const closeForm = () => {
    setCreating(false);
    setEditing(null);
  };

  // 提交：编辑走 update，新建走 create（TaskFormModal 内部处理校验/loading/错误）
  const submitForm: TaskFormSubmit = editing
    ? (name, prompt, scheduleType, value, notifyChannel, notifyTarget) =>
        update(editing.id, name, prompt, scheduleType, value, notifyChannel, notifyTarget)
    : (name, prompt, scheduleType, value, notifyChannel, notifyTarget) =>
        create(name, prompt, scheduleType, value, notifyChannel, notifyTarget);

  const viewHistory = (task: ScheduledTask) => {
    setViewing(task);
    if (unreadOf(task) > 0) markRead(task.id);
  };

  const doDelete = async () => {
    if (!confirmDelete) return;
    const id = confirmDelete.id;
    setConfirmDelete(null);
    await remove(id);
  };

  return (
    <PageContainer
      title="定时任务"
      description="把重复的事交给 Avalon，在指定时间自动执行一次对话。"
    >
      <div className={styles.toolbar}>
        <span className={styles.summary}>
          {loaded ? `${tasks.length} 个任务` : '加载中…'}
          {unread > 0 && <Badge variant="filled">{unread} 未读</Badge>}
        </span>
        <Button variant="primary" onClick={openCreate}>
          ＋ 新建任务
        </Button>
      </div>

      {loaded && tasks.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>还没有定时任务</p>
          <p className={styles.emptyHint}>点右上角「新建任务」，让 Avalon 按时替你做事。</p>
        </div>
      ) : (
        <div className={styles.grid}>
          {tasks.map((task) => {
            const u = unreadOf(task);
            return (
              <div key={task.id} className={styles.taskCard}>
                <div className={styles.cardHead}>
                  <span className={styles.cardName}>{task.name}</span>
                  <div className={styles.badges}>
                    <Badge variant={task.enabled ? 'filled' : 'muted'}>
                      {task.enabled ? '运行中' : '已停用'}
                    </Badge>
                    {task.source === 'agent' && <Badge variant="outline">agent</Badge>}
                    {u > 0 && <Badge variant="filled">{u}</Badge>}
                  </div>
                </div>
                <p className={styles.cardSchedule}>{humanSchedule(task.schedule)}</p>
                <p className={styles.cardPrompt}>{task.prompt}</p>
                <p className={styles.cardMeta}>{lastRunText(task)}</p>
                <div className={styles.cardActions}>
                  <Button variant="secondary" size="sm" onClick={() => viewHistory(task)}>
                    历史
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => openEdit(task)}>
                    编辑
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => toggle(task.id, !task.enabled)}
                  >
                    {task.enabled ? '停用' : '启用'}
                  </Button>
                  <Button variant="danger" size="sm" onClick={() => setConfirmDelete(task)}>
                    删除
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* 新建 / 编辑表单（复用 TaskFormModal） */}
      {(creating || editing) && (
        <TaskFormModal
          key={editing ? editing.id : 'create'}
          title={editing ? '编辑定时任务' : '新建定时任务'}
          initial={editing ?? undefined}
          onSubmit={submitForm}
          onClose={closeForm}
        />
      )}

      {/* 执行历史：从统一主会话中筛出该任务的轮次 */}
      {viewing && (
        <TaskHistoryModal key={viewing.id} task={viewing} onClose={() => setViewing(null)} />
      )}

      {/* 删除确认 */}
      <ConfirmDialog
        open={confirmDelete != null}
        title="删除定时任务"
        description={`确定删除「${confirmDelete?.name ?? ''}」？此操作不可撤销。`}
        confirmText="删除"
        danger
        onConfirm={doDelete}
        onCancel={() => setConfirmDelete(null)}
      />
    </PageContainer>
  );
}

/** 新建/编辑共用的任务表单：initial 为空则新建，否则预填编辑；内部处理校验、loading 与错误显示 */
function TaskFormModal({
  title,
  initial,
  onSubmit,
  onClose,
}: {
  title: string;
  initial?: ScheduledTask;
  onSubmit: TaskFormSubmit;
  onClose: () => void;
}) {
  const init = initial?.schedule;
  const [name, setName] = useState(initial?.name ?? '');
  const [prompt, setPrompt] = useState(initial?.prompt ?? '');
  const [scheduleType, setScheduleType] = useState<ScheduleKind>(init?.type ?? 'once');
  const [notifyChannel, setNotifyChannel] = useState<TaskNotifyChannel>(
    initial?.notify_channel ?? 'none',
  );
  const [notifyTarget, setNotifyTarget] = useState(initial?.notify_target ?? '');
  const [onceValue, setOnceValue] = useState(
    init && init.type === 'once' ? init.at.replace(' ', 'T') : '',
  );
  const [dailyTime, setDailyTime] = useState(init && init.type === 'daily' ? init.time : '');
  const [weeklyDay, setWeeklyDay] = useState(
    init && init.type === 'weekly' ? String(init.weekday) : '1',
  );
  const [weeklyTime, setWeeklyTime] = useState(
    init && init.type === 'weekly' ? init.time : '',
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');

  const submit = async () => {
    setError('');
    let value = '';
    if (scheduleType === 'once') {
      value = onceValue.replace('T', ' ');
      if (!onceValue) {
        setError('请选择执行时间');
        return;
      }
    } else if (scheduleType === 'daily') {
      value = dailyTime;
      if (!dailyTime) {
        setError('请选择执行时间');
        return;
      }
    } else {
      value = `${weeklyDay} ${weeklyTime}`;
      if (!weeklyTime) {
        setError('请选择执行时间');
        return;
      }
    }
    if (!name.trim()) {
      setError('请填写任务名称');
      return;
    }
    if (!prompt.trim()) {
      setError('请填写任务内容');
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(
        name.trim(),
        prompt.trim(),
        scheduleType,
        value,
        notifyChannel,
        notifyTarget.trim() || null,
      );
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      open
      onClose={onClose}
      title={title}
      width={480}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" onClick={submit} disabled={submitting}>
            {submitting ? '保存中…' : '保存'}
          </Button>
        </>
      }
    >
      <div className={styles.form}>
        <Input
          label="任务名称"
          value={name}
          placeholder="例如：每日晨报"
          onChange={(e) => setName(e.currentTarget.value)}
        />
        <label className={styles.fieldLabel} htmlFor="task-prompt">
          任务内容
        </label>
        <textarea
          id="task-prompt"
          className={styles.textarea}
          value={prompt}
          rows={4}
          placeholder="描述要做什么，agent 每次触发时按此执行"
          onChange={(e) => setPrompt(e.currentTarget.value)}
        />

        <span className={styles.fieldLabel}>触发方式</span>
        <div className={styles.scheduleTypeRow}>
          {(
            [
              ['once', '一次性'],
              ['daily', '每天'],
              ['weekly', '每周'],
            ] as [ScheduleKind, string][]
          ).map(([k, label]) => (
            <button
              key={k}
              type="button"
              className={[styles.typeChip, scheduleType === k && styles.typeChipActive]
                .filter(Boolean)
                .join(' ')}
              onClick={() => setScheduleType(k)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className={styles.scheduleValueRow}>
          {scheduleType === 'once' && (
            <input
              type="datetime-local"
              className={styles.field}
              value={onceValue}
              onChange={(e) => setOnceValue(e.currentTarget.value)}
            />
          )}
          {scheduleType === 'daily' && (
            <input
              type="time"
              className={styles.field}
              value={dailyTime}
              onChange={(e) => setDailyTime(e.currentTarget.value)}
            />
          )}
          {scheduleType === 'weekly' && (
            <>
              <select
                className={styles.select}
                value={weeklyDay}
                onChange={(e) => setWeeklyDay(e.currentTarget.value)}
              >
                {WEEKDAYS.map((w, i) => (
                  <option key={w} value={String(i + 1)}>
                    {w}
                  </option>
                ))}
              </select>
              <input
                type="time"
                className={styles.field}
                value={weeklyTime}
                onChange={(e) => setWeeklyTime(e.currentTarget.value)}
              />
            </>
          )}
        </div>

        <span className={styles.fieldLabel}>结果通知</span>
        <div className={styles.scheduleValueRow}>
          <select
            className={styles.select}
            value={notifyChannel}
            onChange={(e) => setNotifyChannel(e.currentTarget.value as TaskNotifyChannel)}
          >
            <option value="none">不通知（只写入会话）</option>
            <option value="desktop">桌面通知</option>
            <option value="feishu">飞书</option>
            <option value="qq">QQ</option>
          </select>
          {(notifyChannel === 'feishu' || notifyChannel === 'qq') && (
            <input
              className={styles.field}
              value={notifyTarget}
              placeholder={
                notifyChannel === 'feishu'
                  ? '会话 chat_id 或 open_id，留空发给主人'
                  : '群号（group:群号）或 QQ 号，留空发给主人'
              }
              onChange={(e) => setNotifyTarget(e.currentTarget.value)}
            />
          )}
        </div>
        <p className={styles.formHint}>
          任务结果同时写入统一会话；若任务内容里已自行调用发送工具，这里选「不通知」以免重复。
        </p>

        {error && <p className={styles.error}>{error}</p>}
      </div>
    </Modal>
  );
}

/** 执行历史模态框：从统一主会话中按 origin.task_id 筛出该任务的轮次回放。
 *  任务内容与模型结论都在主会话时间线里；这里只读，继续对话请回主会话。 */
function TaskHistoryModal({ task, onClose }: { task: ScheduledTask; onClose: () => void }) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    loadTaskMessages(task.id)
      .then((msgs) => {
        if (alive) setMessages(msgs);
      })
      .catch((e) => console.error('加载任务历史失败:', e))
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [task.id]);

  return (
    <Modal open onClose={onClose} title={`执行历史 · ${task.name}`} width={680}>
      <div className={styles.historyView}>
        {!loading && messages.length === 0 ? (
          <p className={styles.formHint}>
            该任务较早的执行已压缩进会话历史，可在主会话中查看；当前未压缩段暂无记录。
          </p>
        ) : (
          <MessageList messages={messages} loading={loading} resetKey={task.id} />
        )}
      </div>
    </Modal>
  );
}
