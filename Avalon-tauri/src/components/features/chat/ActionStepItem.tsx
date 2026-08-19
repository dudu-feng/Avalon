import type { ToolCallRecord } from '../../../types/chat';
import styles from './ActionStepItem.module.css';

export interface ActionStepItemProps {
  tool: ToolCallRecord;
}

function formatArgs(args: unknown): string {
  if (typeof args === 'string') return args;
  if (args === undefined || args === null) return '';
  return JSON.stringify(args);
}

/** 单个工具调用摘要：工具名 + 参数 + 结果（成功/失败着色） */
export function ActionStepItem({ tool }: ActionStepItemProps) {
  const argsText = tool.arguments != null ? formatArgs(tool.arguments) : '';

  return (
    <li className={styles.step}>
      <p className={styles.tool}>
        <span className={styles.toolName}>{tool.toolName}</span>
        {argsText && <span className={styles.args}>{argsText}</span>}
      </p>
      {tool.result != null && (
        <p className={tool.success === false ? styles.resultErr : styles.resultOk}>
          {tool.success === false ? '✗' : tool.success === true ? '✓' : ''} {tool.result}
        </p>
      )}
    </li>
  );
}
