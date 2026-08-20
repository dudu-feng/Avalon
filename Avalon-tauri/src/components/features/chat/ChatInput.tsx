import { useRef, useState } from 'react';
import type { ChangeEvent, KeyboardEvent } from 'react';
import { Button } from '../../ui';
import styles from './ChatInput.module.css';

export interface ModelOption {
  name: string;
  modelname: string;
}

export interface ChatInputProps {
  onSubmit: (text: string) => void;
  onStop: () => void;
  isBusy?: boolean;
  models: ModelOption[];
  activeModel: string;
  onModelChange: (name: string) => void;
}

/** textarea 自动增长的封顶高度（与 CSS 的 max-height 一致） */
const MAX_HEIGHT = 160;

export function ChatInput({
  onSubmit,
  onStop,
  isBusy = false,
  models,
  activeModel,
  onModelChange,
}: ChatInputProps) {
  const [value, setValue] = useState('');
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
        <span className={styles.hint}>Enter 发送 · Shift+Enter 换行</span>
        <div className={styles.right}>
          <select
            className={styles.model}
            value={activeModel}
            onChange={(e) => onModelChange(e.currentTarget.value)}
            aria-label="选择模型"
          >
            {models.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
              </option>
            ))}
          </select>
          <Button
            variant={isBusy ? 'secondary' : 'primary'}
            onClick={isBusy ? onStop : submit}
            disabled={!isBusy && !value.trim()}
          >
            {isBusy ? '停止' : '发送'}
          </Button>
        </div>
      </div>
    </div>
  );
}
