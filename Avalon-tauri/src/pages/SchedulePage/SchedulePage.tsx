// 定时任务页面：任务卡片列表 + 创建表单（Modal）+ 执行历史（Modal 复用 MessageList）。
//
// 任务 = 自动驱动的会话：执行过程落 session（channel = task.id，ensure_active 自动建会话），
// 点「历史」读取该会话消息并以会话式回放；新执行经全局事件 task-finished 刷新未读角标。

import { useState } from 'react';
import { PageContainer, Badge, Button, Input, Modal, ConfirmDialog } from '../../components/ui';
import { MessageList, ChatInput, useChat } from '../../components/features/chat';
import { useScheduler } from '../../hooks/useSchedulerStore';
import type { ScheduleKind } from '../../lib/schedulerApi';
import type { ScheduledTask, ScheduleType } from '../../types/scheduler';
import styles from './SchedulePage.module.css';

const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'];

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
  return `上次执行 ${last.triggered_at} · ${last.status === 'succeeded' ? '成功' : '失败'}`;
}

export function SchedulePage() {
  const { tasks, unread, loaded, create, remove, toggle, markRead } = useScheduler();

  // 创建表单
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [scheduleType, setScheduleType] = useState<ScheduleKind>('once');
  const [onceValue, setOnceValue] = useState('');
  const [dailyTime, setDailyTime] = useState('');
  const [weeklyDay, setWeeklyDay] = useState('1');
  const [weeklyTime, setWeeklyTime] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');

  // 删除确认
  const [confirmDelete, setConfirmDelete] = useState<ScheduledTask | null>(null);

  // 执行历史（内部 TaskHistoryModal 用 useChat 驱动，可继续对话）
  const [viewing, setViewing] = useState<ScheduledTask | null>(null);

  // 重置创建表单（打开 / 关闭 / 创建成功后统一清空，避免残留）
  const resetForm = () => {
    setName('');
    setPrompt('');
    setScheduleType('once');
    setOnceValue('');
    setDailyTime('');
    setWeeklyDay('1');
    setWeeklyTime('');
    setError('');
  };

  const openCreate = () => {
    resetForm();
    setCreating(true);
  };

  const closeCreate = () => {
    setCreating(false);
    resetForm();
  };

  const submitCreate = async () => {
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
      await create(name.trim(), prompt.trim(), scheduleType, value);
      closeCreate();
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

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

      {/* 创建表单 */}
      <Modal
        open={creating}
        onClose={closeCreate}
        title="新建定时任务"
        width={480}
        footer={
          <>
            <Button variant="secondary" onClick={closeCreate}>
              取消
            </Button>
            <Button variant="primary" onClick={submitCreate} disabled={submitting}>
              {submitting ? '创建中…' : '创建'}
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

          {error && <p className={styles.error}>{error}</p>}
        </div>
      </Modal>

      {/* 执行历史：独立会话视图，用户可继续对话（channel = task.id） */}
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

/** 执行历史模态框：以 task.id 为 channel 的独立会话，复用 useChat + MessageList + ChatInput，用户可继续对话 */
function TaskHistoryModal({ task, onClose }: { task: ScheduledTask; onClose: () => void }) {
  const { messages, isBusy, send, stop, loading, hasEarlier, loadingEarlier, loadEarlier } =
    useChat({ channelName: task.id });

  return (
    <Modal open onClose={onClose} title={`执行历史 · ${task.name}`} width={680}>
      <div className={styles.historyView}>
        <MessageList
          messages={messages}
          loading={loading}
          resetKey={task.id}
          hasEarlier={hasEarlier}
          loadingEarlier={loadingEarlier}
          onLoadEarlier={loadEarlier}
        />
        <ChatInput onSubmit={send} onStop={stop} isBusy={isBusy} />
      </div>
    </Modal>
  );
}
