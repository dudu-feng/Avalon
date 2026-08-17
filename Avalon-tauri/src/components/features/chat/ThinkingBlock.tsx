import { useState } from 'react';
import styles from './ThinkingBlock.module.css';

export interface ThinkingBlockProps {
  thought: string;
}

export function ThinkingBlock({ thought }: ThinkingBlockProps) {
  const [open, setOpen] = useState(true);

  return (
    <div className={styles.block}>
      <button
        type="button"
        className={styles.toggle}
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        {open ? '▾ 思考过程' : '▸ 思考过程'}
      </button>
      {open && <p className={styles.content}>{thought}</p>}
    </div>
  );
}
