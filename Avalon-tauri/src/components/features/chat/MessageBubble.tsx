import { memo, useEffect, useState } from 'react';
import { openPath } from '@tauri-apps/plugin-opener';
import type { ChatMessage, MessageAttachment, MessageOrigin } from '../../../types/chat';
import { getThumbnailDataUrl, resolveFilePath } from '../../../lib/fileApi';
import { ThinkingBlock } from './ThinkingBlock';
import { ActionStepItem } from './ActionStepItem';
import { MarkdownContent } from './MarkdownContent';
import styles from './MessageBubble.module.css';

export interface MessageBubbleProps {
  message: ChatMessage;
}

const SOURCE_LABELS: Record<string, string> = {
  desktop: '桌面',
  feishu: '飞书',
  qq: 'QQ',
  task: '定时任务',
};

/** 历史时间串只取「月-日 时:分」，兼容 `YYYY-MM-DD HH:MM:SS` 与 RFC3339 */
function shortTime(time: string): string {
  const m = /(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/.exec(time);
  return m ? `${m[2]}-${m[3]} ${m[4]}:${m[5]}` : time;
}

/** 来源 + 信任 + 时间的元信息行（桌面也显示，保持统一时间线可溯源） */
function MetaLine({ origin, time }: { origin?: MessageOrigin | null; time?: string }) {
  const label = origin ? SOURCE_LABELS[origin.source] : '';
  const isExternal =
    !!origin && origin.source !== 'desktop' && origin.trust !== undefined && origin.trust !== 'owner';
  if (!label && !isExternal && !time) return null;
  return (
    <div className={styles.meta}>
      {label && (
        <span className={styles.sourceBadge} data-source={origin?.source}>
          {label}
        </span>
      )}
      {isExternal && <span className={styles.externalBadge}>外部</span>}
      {time && <span className={styles.timeText}>{shortTime(time)}</span>}
    </div>
  );
}

/** 定时任务消息卡片：标题 = 任务名，正文默认折叠；模型结论作为后续 assistant 气泡展示 */
function TaskCard({ message }: { message: Extract<ChatMessage, { role: 'user' }> }) {
  const [open, setOpen] = useState(false);
  const name = message.origin?.task_name || '定时任务';
  return (
    <div className={styles.taskCard}>
      <div className={styles.taskHead}>
        <span className={styles.taskIcon}>⏱</span>
        <span className={styles.taskTitle}>{name}</span>
        {message.time && <span className={styles.timeText}>{shortTime(message.time)}</span>}
      </div>
      <button type="button" className={styles.taskToggle} onClick={() => setOpen((v) => !v)}>
        {open ? '收起任务内容' : '展开任务内容'}
      </button>
      {open && message.content && <p className={styles.text}>{message.content}</p>}
    </div>
  );
}

/** 单张图片附件：先取缩略图，点击用系统程序打开原图 */
function AttachmentThumb({ attachment }: { attachment: MessageAttachment }) {
  const [url, setUrl] = useState('');

  useEffect(() => {
    let alive = true;
    getThumbnailDataUrl(attachment.path)
      .then((u) => {
        if (alive) setUrl(u);
      })
      .catch((e) => console.error('加载缩略图失败:', e));
    return () => {
      alive = false;
    };
  }, [attachment.path]);

  const open = async () => {
    try {
      await openPath(await resolveFilePath(attachment.path));
    } catch (e) {
      console.error('打开图片失败:', e);
    }
  };

  const name = attachment.original_name || '图片';
  return (
    <button type="button" className={styles.attachmentItem} onClick={open} title={`打开 ${name}`}>
      {url ? (
        <img className={styles.attachmentImage} src={url} alt={name} loading="lazy" />
      ) : (
        <span className={styles.attachmentFallback}>图片</span>
      )}
    </button>
  );
}

function MessageBubbleImpl({ message }: MessageBubbleProps) {
  // tool 消息：独立折叠卡片（左对齐，不套气泡外壳，参数/结果自包含）
  if (message.role === 'tool') {
    return (
      <div className={`${styles.row} ${styles.assistant}`}>
        <MetaLine origin={message.origin} time={message.time} />
        <div className={styles.tool}>
          <ActionStepItem tool={message.tool} />
        </div>
      </div>
    );
  }

  // compress 消息：压缩上下文的过程提示条（居中，不套气泡外壳，不入历史）
  if (message.role === 'compress') {
    const label =
      message.status === 'running'
        ? '正在压缩上下文…'
        : message.status === 'done'
          ? '上下文已压缩'
          : `压缩上下文失败${message.error ? `：${message.error}` : ''}`;
    return (
      <div className={styles.compressRow}>
        {message.status === 'running' ? (
          <span className={styles.compressSpinner} />
        ) : (
          <span className={styles.compressIcon}>{message.status === 'done' ? '✓' : '✗'}</span>
        )}
        <span className={styles.compressLabel} data-status={message.status}>
          {label}
        </span>
      </div>
    );
  }

  const isUser = message.role === 'user';
  const isError = message.status === 'error';
  const isStreaming = message.status === 'streaming';

  const rowClass = [styles.row, isUser ? styles.user : styles.assistant, isError && styles.error]
    .filter(Boolean)
    .join(' ');

  if (isUser) {
    // 定时任务不用普通用户气泡：以任务卡片呈现，避免把任务名当成用户发言
    if (message.origin?.source === 'task') {
      return (
        <div className={`${styles.row} ${styles.assistant}`}>
          <TaskCard message={message} />
        </div>
      );
    }
    return (
      <div className={rowClass}>
        <MetaLine origin={message.origin} time={message.time} />
        <div className={styles.bubble}>
          {message.attachments && message.attachments.length > 0 && (
            <div className={styles.attachmentGrid}>
              {message.attachments.map((a) => (
                <AttachmentThumb key={`${a.path}-${a.sha256}`} attachment={a} />
              ))}
            </div>
          )}
          {message.content && <p className={styles.text}>{message.content}</p>}
        </div>
      </div>
    );
  }

  return (
    <div className={rowClass}>
      <MetaLine origin={message.origin} time={message.time} />
      <div className={styles.bubble}>
        {message.thought && <ThinkingBlock thought={message.thought} />}
        {isError ? (
          <p className={styles.text}>{message.error}</p>
        ) : (
          (message.content || isStreaming) && (
            <MarkdownContent streaming={isStreaming}>{message.content}</MarkdownContent>
          )
        )}
      </div>
    </div>
  );
}

// 历史消息对象在流式过程中保持引用不变，memo 让它们完全跳过重渲染；
// 只有正在增长的流式气泡需要重新执行 Markdown 解析。
export const MessageBubble = memo(MessageBubbleImpl);
