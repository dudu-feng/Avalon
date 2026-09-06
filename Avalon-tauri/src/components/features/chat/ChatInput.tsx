import { useRef, useState } from 'react';
import type { ChangeEvent, KeyboardEvent } from 'react';
import { Button, CircleProgress, ConfirmDialog, Dropdown, Tooltip } from '../../ui';
import type { ContextUsage } from '../../../types/chat';
import styles from './ChatInput.module.css';

export interface ModelOption {
  name: string;
  modelname: string;
}

export interface ChatInputProps {
  onSubmit: (text: string) => void;
  onStop: () => void;
  isBusy?: boolean;
  models?: ModelOption[];
  activeModel?: string;
  onModelChange?: (name: string) => void;
  contextUsage?: ContextUsage | null;
  /** 主动压缩当前上下文（进度圈点击确认后触发） */
  onCompress?: () => Promise<void> | void;
  /** 是否正在主动压缩（进度圈禁用 + 防止重复触发） */
  compressing?: boolean;
}

/** textarea 自动增长的封顶高度（与 CSS 的 max-height 一致） */
const MAX_HEIGHT = 160;

export function ChatInput({
  onSubmit,
  onStop,
  isBusy = false,
  models = [],
  activeModel = '',
  onModelChange = () => {},
  contextUsage,
  onCompress,
  compressing = false,
}: ChatInputProps) {
  const [value, setValue] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  const fieldRef = useRef<HTMLTextAreaElement>(null);

  // 自动增长：先复位再按内容撑高，封顶后内部滚动
  const adjustHeight = () => {
    const field = fieldRef.current;
    if (!field) return;
    field.style.height = 'auto';
    field.style.height = `${Math.min(field.scrollHeight, MAX_HEIGHT)}px`;
  };

  const submit = () => {
    const trimmed = value.trim();
    if (!trimmed || isBusy) return;
    onSubmit(trimmed);
    setValue('');
    // 清空后复位为单行（height:auto 随空内容缩回）
    if (fieldRef.current) fieldRef.current.style.height = 'auto';
  };

  // 确认框确认：关弹窗后触发主动压缩（错误仅记日志，不打断输入流程）
  const handleCompress = async () => {
    setConfirmOpen(false);
    if (!onCompress || compressing) return;
    try {
      await onCompress();
    } catch (e) {
      console.error('压缩上下文失败:', e);
    }
  };

  const onChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    setValue(e.currentTarget.value);
    adjustHeight();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter 发送（Shift+Enter 换行）；IME 组合态中的 Enter 不触发
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <>
      <div className={styles.card}>
        <textarea
          ref={fieldRef}
          className={styles.field}
          value={value}
          rows={1}
          placeholder="输入消息，Enter 发送，Shift+Enter 换行…"
          onChange={onChange}
          onKeyDown={onKeyDown}
        />
        <div className={styles.toolbar}>
          <div className={styles.left}>
            {contextUsage && (
              <Tooltip
                label={`上下文 ${contextUsage.used_tokens} / ${contextUsage.threshold} tokens · 点击压缩`}
              >
                <button
                  type="button"
                  className={styles.usageButton}
                  onClick={() => setConfirmOpen(true)}
                  disabled={compressing}
                  title="压缩当前上下文"
                  aria-label="压缩当前上下文"
                >
                  <CircleProgress
                    value={contextUsage.used_tokens}
                    max={contextUsage.threshold}
                    size={18}
                    strokeWidth={3}
                    label=""
                  />
                </button>
              </Tooltip>
            )}
            <span className={styles.hint}>Enter 发送 · Shift+Enter 换行</span>
          </div>
          <div className={styles.right}>
            {models.length > 0 && (
              <Dropdown
                options={models.map((m) => ({ value: m.name, label: m.name }))}
                value={activeModel}
                onChange={onModelChange}
                align="end"
              />
            )}
            <Button
              size="sm"
              variant={isBusy ? 'secondary' : 'primary'}
              onClick={isBusy ? onStop : submit}
              disabled={!isBusy && !value.trim()}
            >
              {isBusy ? '停止' : '发送'}
            </Button>
          </div>
        </div>
      </div>
      <ConfirmDialog
        open={confirmOpen}
        title="压缩上下文？"
        description="将当前会话的未压缩消息压缩为摘要，释放上下文空间。压缩后的原始内容仍会保留在历史记录中。"
        confirmText="压缩"
        onConfirm={handleCompress}
        onCancel={() => setConfirmOpen(false)}
      />
    </>
  );
}
