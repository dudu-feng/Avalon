import { useState } from 'react';
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

/** 单个工具调用步骤卡片：默认折叠只显「状态 + 工具名」，点击展开「参数 + 结果」 */
export function ActionStepItem({ tool }: ActionStepItemProps) {
  // 执行中默认展开（便于观察进行中的调用），完成/失败后可由用户手动折叠
  const [open, setOpen] = useState(tool.status === 'running');

  const icon = tool.status === 'running' ? '⏳' : tool.status === 'success' ? '✓' : '✗';
  const argsText = tool.arguments != null ? formatArgs(tool.arguments) : '';

  return (
    <div className={styles.card}>
      <button
        type="button"
        className={styles.cardHead}
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
      >
        <span className={styles.icon} data-status={tool.status}>
          {icon}
        </span>
        <span className={styles.toolName}>{tool.toolName}</span>
        <span className={styles.chevron}>{open ? '▾' : '▸'}</span>
      </button>
      {open && (
        <div className={styles.cardBody}>
          {argsText && <p className={styles.args}>{argsText}</p>}
          {tool.result != null && (
            <p className={tool.status === 'error' ? styles.resultErr : styles.resultOk}>
              {tool.result}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
