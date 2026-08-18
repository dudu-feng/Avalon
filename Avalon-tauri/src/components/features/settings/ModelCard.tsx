import { Button, Badge } from '../../ui';
import type { ModelConfig } from '../../../types/config';
import { ModelForm } from './ModelForm';
import styles from './ModelCard.module.css';

export interface ModelCardProps {
  model: ModelConfig;
  isActive: boolean;
  onChange: (next: ModelConfig) => void;
  onRemove: () => void;
  onSetActive: () => void;
}

/** 模型列表项：头部（标识 + 活跃标记 + 操作）+ 内联编辑表单 */
export function ModelCard({ model, isActive, onChange, onRemove, onSetActive }: ModelCardProps) {
  return (
    <div className={styles.card}>
      <div className={styles.head}>
        <div className={styles.identity}>
          <span className={styles.name}>{model.name || '未命名模型'}</span>
          {isActive ? (
            <Badge variant="filled">active</Badge>
          ) : (
            <Badge variant="muted">未启用</Badge>
          )}
        </div>
        <div className={styles.actions}>
          <Button variant="ghost" size="sm" onClick={onSetActive} disabled={isActive}>
            设为活跃
          </Button>
          <Button variant="ghost" size="sm" onClick={onRemove}>
            删除
          </Button>
        </div>
      </div>
      <ModelForm model={model} onChange={onChange} />
    </div>
  );
}
