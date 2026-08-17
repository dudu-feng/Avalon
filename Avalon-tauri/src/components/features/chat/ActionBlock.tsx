import { useState } from 'react';
import type { ActionBlock as ActionBlockModel } from '../../../types/chat';
import { ActionStepItem } from './ActionStepItem';
import styles from './ActionBlock.module.css';

export interface ActionBlockProps {
  block: ActionBlockModel;
}

export function ActionBlock({ block }: ActionBlockProps) {
  const [open, setOpen] = useState(true);

  return (
    <div className={styles.block}>
      <button
        type="button"
        className={styles.toggle}
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        <span>{open ? '▾' : '▸'}</span>
        <span>动作层 · {block.target || '执行任务'}</span>
        <span className={styles.count}>{block.steps.length} 步</span>
      </button>
      {open && (
        <ol className={styles.steps}>
          {block.steps.map((step, i) => (
            <ActionStepItem key={i} step={step} />
          ))}
        </ol>
      )}
    </div>
  );
}
