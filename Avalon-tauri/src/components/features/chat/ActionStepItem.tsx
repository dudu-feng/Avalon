import type { ActionStepRecord } from '../../../types/chat';
import styles from './ActionStepItem.module.css';

export interface ActionStepItemProps {
  step: ActionStepRecord;
}

function formatArgs(args: unknown): string {
  if (typeof args === 'string') return args;
  if (args === undefined || args === null) return '';
  return JSON.stringify(args);
}

export function ActionStepItem({ step }: ActionStepItemProps) {
  return (
    <li className={styles.step}>
      {step.analysis && <p className={styles.analysis}>{step.analysis}</p>}

      {step.next === 'tool_call' && (
        <>
          {step.toolCall && (
            <p className={styles.tool}>
              <span className={styles.toolName}>{step.toolCall.toolName}</span>
              <span className={styles.args}>{formatArgs(step.toolCall.arguments)}</span>
            </p>
          )}
          {step.toolResult && (
            <p className={step.toolResult.success ? styles.resultOk : styles.resultErr}>
              {step.toolResult.success ? '✓' : '✗'} {step.toolResult.result}
            </p>
          )}
        </>
      )}

      {step.next === 'sub_analysis' && step.subAnalysis && (
        <p className={styles.sub}>{step.subAnalysis}</p>
      )}

      {step.next === 'finished' && step.finished && (
        <p className={styles.usage}>
          完成 · 本轮消耗 {step.finished.tokenUsage.total_tokens} tokens
        </p>
      )}
    </li>
  );
}
