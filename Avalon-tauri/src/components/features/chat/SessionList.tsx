// 会话历史列表（Chat 页左侧面板）
//
// 职责：展示会话历史（active 置顶 + 归档按时间分组）、切换 / 重命名 / 删除 / 新建。
// 时间分组：今天 / 昨天 / 过去 7 天 / 更早（由 created_at epoch 秒推导）。
// 交互：点击条目切换；hover 显示重命名（✎）与删除（×）按钮；重命名走内联输入，删除走确认框。

import { useState, type KeyboardEvent, type MouseEvent } from 'react';
import { Button, ConfirmDialog, Skeleton } from '../../ui';
import type { SessionMeta } from '../../../types/chat';
import styles from './SessionList.module.css';

export interface SessionListProps {
  sessions: SessionMeta[];
  activeId: string;
  /** 列表加载骨架（首次加载） */
  loading?: boolean;
  onSelect: (id: string) => void;
  onNew: () => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
}

/** 时间分组标签（由 created_at epoch 秒推导） */
function timeGroupLabel(epochSec: number): string {
  if (!epochSec) return '更早';
  const now = new Date();
  const d = new Date(epochSec * 1000);
  const startOfDay = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000);
  if (dayDiff <= 0) return '今天';
  if (dayDiff === 1) return '昨天';
  if (dayDiff <= 7) return '过去 7 天';
  return '更早';
}

/** 条目副标题：相对时间（今天显示 HH:MM）+ 消息量 */
function formatMeta(epochSec: number, messageCount: number): string {
  const parts: string[] = [];
  if (epochSec) {
    const d = new Date(epochSec * 1000);
    const now = new Date();
    const startOfDay = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000);
    if (dayDiff <= 0) {
      parts.push(`${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`);
    } else if (dayDiff === 1) {
      parts.push('昨天');
    } else if (dayDiff <= 7) {
      parts.push(`${dayDiff} 天前`);
    } else {
      parts.push(`${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`);
    }
  }
  parts.push(`${messageCount} 条消息`);
  return parts.join(' · ');
}

/** 空标题回退：active 用「新会话」，归档回退时间戳 */
function fallbackTitle(s: SessionMeta): string {
  if (s.title) return s.title;
  if (s.status === 'active') return '新会话';
  if (s.created_at) {
    const d = new Date(s.created_at * 1000);
    return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  }
  return '未命名会话';
}

interface Group {
  label: string;
  items: SessionMeta[];
}

/** 归档会话按时间分组（后端已按时间倒序，分组顺序 = 出现顺序） */
function groupSessions(archived: SessionMeta[]): Group[] {
  const groups: Group[] = [];
  const map = new Map<string, SessionMeta[]>();
  for (const s of archived) {
    const label = timeGroupLabel(s.created_at);
    if (!map.has(label)) map.set(label, []);
    map.get(label)!.push(s);
  }
  for (const [label, items] of map) groups.push({ label, items });
  return groups;
}

export function SessionList({
  sessions,
  activeId,
  loading = false,
  onSelect,
  onNew,
  onRename,
  onDelete,
}: SessionListProps) {
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<SessionMeta | null>(null);

  const active = sessions.find((s) => s.status === 'active');
  const archived = sessions.filter((s) => s.status !== 'active');
  const groups = groupSessions(archived);

  // 进入重命名：记录 id + 初值
  const startRename = (s: SessionMeta) => {
    setRenamingId(s.id);
    setRenameValue(s.title || fallbackTitle(s));
  };

  const commitRename = () => {
    if (renamingId) {
      const title = renameValue.trim();
      if (title) onRename(renamingId, title);
    }
    setRenamingId(null);
    setRenameValue('');
  };

  const onRenameKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      setRenamingId(null);
      setRenameValue('');
    }
  };

  // 点击条目主体：非重命名态才切换；动作按钮 stopPropagation 防冒泡
  const onItemClick = (id: string) => {
    if (renamingId === id) return;
    if (id !== activeId) onSelect(id);
  };

  const stop = (e: MouseEvent) => e.stopPropagation();

  const renderItem = (s: SessionMeta) => {
    const isActive = s.id === activeId;
    const isRenaming = renamingId === s.id;

    return (
      <div
        key={s.id}
        className={[styles.item, isActive ? styles.active : '', isRenaming ? styles.renaming : '']
          .filter(Boolean)
          .join(' ')}
        onClick={() => onItemClick(s.id)}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !isRenaming) onItemClick(s.id);
        }}
      >
        {isRenaming ? (
          <input
            className={styles.renameInput}
            value={renameValue}
            autoFocus
            onChange={(e) => setRenameValue(e.currentTarget.value)}
            onBlur={commitRename}
            onKeyDown={onRenameKeyDown}
            onClick={stop}
            aria-label="重命名会话"
          />
        ) : (
          <div className={styles.itemMain}>
            <span className={styles.itemTitle} title={s.title || fallbackTitle(s)}>
              {fallbackTitle(s)}
            </span>
            <span className={styles.itemMeta}>{formatMeta(s.created_at, s.message_count)}</span>
          </div>
        )}
        {!isRenaming && (
          <div className={styles.itemActions}>
            <button
              type="button"
              className={styles.action}
              title="重命名"
              aria-label="重命名"
              onClick={(e) => {
                stop(e);
                startRename(s);
              }}
            >
              ✎
            </button>
            {s.status !== 'active' && (
              <button
                type="button"
                className={styles.action}
                title="删除"
                aria-label="删除"
                onClick={(e) => {
                  stop(e);
                  setDeleteTarget(s);
                }}
              >
                ×
              </button>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <aside className={styles.panel}>
      <header className={styles.header}>
        <span className={styles.heading}>会话</span>
        <Button variant="ghost" size="sm" onClick={onNew} title="新建会话">
          ＋ 新建
        </Button>
      </header>

      <div className={styles.list}>
        {loading ? (
          <div className={styles.skeletonWrap}>
            <Skeleton className={styles.skeletonItem} />
            <Skeleton className={styles.skeletonItem} />
            <Skeleton className={styles.skeletonItem} />
          </div>
        ) : (
          <>
            {active && renderItem(active)}
            {groups.map((g) => (
              <div key={g.label} className={styles.group}>
                <div className={styles.groupLabel}>{g.label}</div>
                {g.items.map(renderItem)}
              </div>
            ))}
            {!active && groups.length === 0 && (
              <div className={styles.empty}>暂无会话记录</div>
            )}
          </>
        )}
      </div>

      <ConfirmDialog
        open={deleteTarget !== null}
        title="删除会话"
        description={`确定删除「${deleteTarget ? fallbackTitle(deleteTarget) : ''}」吗？该会话的消息与记忆向量将一并清除，且不可恢复。`}
        confirmText="删除"
        danger
        onCancel={() => setDeleteTarget(null)}
        onConfirm={() => {
          if (deleteTarget) onDelete(deleteTarget.id);
          setDeleteTarget(null);
        }}
      />
    </aside>
  );
}
