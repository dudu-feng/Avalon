import type { ToolCallRecord } from '../../../types/chat';
import { ActionStepItem } from './ActionStepItem';
import styles from './ActionBlock.module.css';

export interface ActionBlockProps {
  tools: ToolCallRecord[];
}

/** 工具调用摘要区：平铺展示本轮所有工具调用（工具名 + 结果摘要） */
export function ActionBlock({ tools }: ActionBlockProps) {
  return (
    <div className={styles.block}>
      <ul className={styles.steps}>
        {tools.map((tool, i) => (
          <ActionStepItem key={i} tool={tool} />
        ))}
      </ul>
    </div>
  );
}
