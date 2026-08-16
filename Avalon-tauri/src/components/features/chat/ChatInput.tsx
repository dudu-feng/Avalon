import { useState } from 'react';
import { Button } from '../../ui';
import styles from './ChatInput.module.css';

export interface ChatInputProps {
  onSubmit: (text: string) => void;
  disabled?: boolean;
}

export function ChatInput({ onSubmit, disabled = false }: ChatInputProps) {
  const [value, setValue] = useState('');

  const submit = () => {
    const trimmed = value.trim();
    if (!trimmed || disabled) return;
    onSubmit(trimmed);
    setValue('');
  };

  return (
    <div className={styles.bar}>
      <textarea
        className={styles.field}
        value={value}
        placeholder="输入消息，Enter 发送，Shift+Enter 换行…"
        rows={1}
        onChange={(e) => setValue(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            submit();
          }
        }}
      />
      <Button variant="primary" onClick={submit} disabled={disabled || !value.trim()}>
        {disabled ? '发送中…' : '发送'}
      </Button>
    </div>
  );
}
