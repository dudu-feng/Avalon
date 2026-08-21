// 会话历史列表（Chat 页左侧面板）
//
// 职责：展示会话历史（统一按时间分组、不置顶）、滑动指示器高亮当前会话、切换 / 重命名 / 删除 / 新建。
// 时间分组：今天 / 昨天 / 过去 7 天 / 更早（由 created_at epoch 秒推导）。
// 交互：点击条目切换；hover 显示「…」按钮，点击展开操作菜单（重命名 / 删除）；重命名走内联输入，删除走确认框。
// 当前会话用滑动指示器 + 加粗标题标识（不置顶），切换时指示器平滑滑到目标条目。

import { useLayoutEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import { Button, ConfirmDialog, Menu, Skeleton } from '../../ui';
import type { MenuItem } from '../../ui';
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

/** 条目副标题：相对时间（今天显示 HH:MM，否则昨天 / N 天前 / 日期） */
function formatMeta(epochSec: number): string {
  if (!epochSec) return '';
  const d = new Date(epochSec * 1000);
  const now = new Date();
  const startOfDay = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const dayDiff = Math.round((startOfDay(now) - startOfDay(d)) / 86400000);
  if (dayDiff <= 0) {
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  }
  if (dayDiff === 1) return '昨天';
  if (dayDiff <= 7) return `${dayDiff} 天前`;
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`;
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

/** 全部会话（含 active）按时间分组（后端已按时间倒序，分组顺序 = 出现顺序） */
function groupSessions(sessions: SessionMeta[]): Group[] {
  const groups: Group[] = [];
  const map = new Map<string, SessionMeta[]>();
  for (const s of sessions) {
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

  // 滑动指示器：绝对定位的高亮块，位置由 active 条目实时测量
  const listRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef(new Map<string, HTMLDivElement>());
  const [indicator, setIndicator] = useState({ top: 0, height: 0, visible: false });
  const [ready, setReady] = useState(false); // 首次定位后才启用过渡，避免首帧从顶部滑入

  const groups = groupSessions(sessions);

  // activeId / 列表 / 重命名态变化时，重新测量指示器位置
  useLayoutEffect(() => {
    const list = listRef.current;
    const item = activeId ? itemRefs.current.get(activeId) : undefined;
    if (!list || !item) {
      setIndicator((p) => ({ ...p, visible: false }));
      return;
    }
    const lr = list.getBoundingClientRect();
    const ir = item.getBoundingClientRect();
    setIndicator({
      top: ir.top - lr.top - list.clientTop + list.scrollTop,
      height: ir.height,
      visible: true,
    });
    requestAnimationFrame(() => setReady(true));
  }, [activeId, sessions, renamingId]);

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

  // 点击条目主体：非重命名态才切换；「…」按钮容器 stopPropagation 防冒泡
  const onItemClick = (id: string) => {
    if (renamingId === id) return;
    if (id !== activeId) onSelect(id);
  };

  const stop = (e: MouseEvent) => e.stopPropagation();

  const renderItem = (s: SessionMeta) => {
    const isActive = s.id === activeId;
    const isRenaming = renamingId === s.id;

    const menuItems: MenuItem[] = [
      { label: '重命名', onSelect: () => startRename(s) },
    ];
    if (s.status !== 'active') {
      menuItems.push({ label: '删除', danger: true, onSelect: () => setDeleteTarget(s) });
    }

    return (
      <div
        key={s.id}
        ref={(el) => {
          if (el) itemRefs.current.set(s.id, el);
          else itemRefs.current.delete(s.id);
        }}
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
            <span className={styles.itemMeta}>{formatMeta(s.created_at)}</span>
          </div>
        )}
        {!isRenaming && (
          <div className={styles.itemActions} onClick={stop}>
            <Menu
              align="end"
              ariaLabel="会话操作"
              className={styles.moreBtn}
              trigger="…"
              items={menuItems}
            />
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

      <div className={styles.list} ref={listRef}>
        <div
          className={[styles.indicator, ready ? styles.ready : ''].filter(Boolean).join(' ')}
          style={{ top: indicator.top, height: indicator.height, opacity: indicator.visible ? 1 : 0 }}
          aria-hidden
        />
        {loading ? (
          <div className={styles.skeletonWrap}>
            <Skeleton className={styles.skeletonItem} />
            <Skeleton className={styles.skeletonItem} />
            <Skeleton className={styles.skeletonItem} />
          </div>
        ) : (
          <>
            {groups.map((g) => (
              <div key={g.label} className={styles.group}>
                <div className={styles.groupLabel}>{g.label}</div>
                {g.items.map(renderItem)}
              </div>
            ))}
            {sessions.length === 0 && <div className={styles.empty}>暂无会话记录</div>}
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
