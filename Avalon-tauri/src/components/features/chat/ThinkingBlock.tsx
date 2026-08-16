import { useState } from 'react';
import styles from './ThinkingBlock.module.css';

export interface ThinkingBlockProps {
  thought: string;
}

export function ThinkingBlock({ thought }: ThinkingBlockProps) {
  const [open, setOpen] = useState(false);

  return (
    <div className={styles.block}>
      <button
        type="button"
        className={styles.toggle}
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        {open ? '收起思考' : '展开思考'}
      </button>
      {open && <p className={styles.content}>{thought}</p>}
    </div>
  );
}
