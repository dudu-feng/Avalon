import { Input, Switch } from '../../ui';
import type { ModelConfig } from '../../../types/config';
import styles from './ModelForm.module.css';

export interface ModelFormProps {
  model: ModelConfig;
  onChange: (next: ModelConfig) => void;
}

/** 空输入表示「跟随全局兜底」，不写入 0 或 NaN */
function optionalPositiveInt(raw: string): number | undefined {
  const n = Number(raw);
  return raw.trim() !== '' && Number.isInteger(n) && n > 0 ? n : undefined;
}

/** 单模型内联编辑表单：name / url / key / modelname */
export function ModelForm({ model, onChange }: ModelFormProps) {
  function patch(p: Partial<ModelConfig>) {
    onChange({ ...model, ...p });
  }

  return (
    <div className={styles.grid}>
      <Input
        label="名称 name"
        value={model.name}
        placeholder="唯一标识，如 deepseek"
        onChange={(e) => patch({ name: e.currentTarget.value })}
      />
      <Input
        label="模型名 modelname"
        value={model.modelname}
        placeholder="实际模型名，如 deepseek-v4-flash"
        onChange={(e) => patch({ modelname: e.currentTarget.value })}
      />
      <Input
        label="接口地址 url"
        value={model.url}
        placeholder="https://api.deepseek.com"
        onChange={(e) => patch({ url: e.currentTarget.value })}
      />
      <Input
        label="API Key key"
        type="password"
        value={model.key}
        placeholder="支持环境变量 AVALON_LLM_API_KEY 覆盖"
        onChange={(e) => patch({ key: e.currentTarget.value })}
      />
      <Input
        label="上下文窗口 context_window"
        type="number"
        min={1}
        step={1}
        value={model.context_window ?? ''}
        placeholder="留空使用会话记忆里的兜底值"
        onChange={(e) => patch({ context_window: optionalPositiveInt(e.currentTarget.value) })}
      />
      <Input
        label="输出预留 max_output_tokens"
        type="number"
        min={1}
        step={1}
        value={model.max_output_tokens ?? ''}
        placeholder="留空使用会话记忆里的兜底值"
        onChange={(e) => patch({ max_output_tokens: optionalPositiveInt(e.currentTarget.value) })}
      />
      <div className={styles.visionRow}>
        <div className={styles.visionText}>
          <span className={styles.visionLabel}>支持图片输入 supports_vision</span>
          <span className={styles.visionHint}>
            开启后该模型可直接接收图片（模型本身须为多模态）；还需在「视觉」页启用图片输入
          </span>
        </div>
        <Switch
          checked={model.supports_vision}
          onChange={(checked) => patch({ supports_vision: checked })}
          aria-label={`${model.name || '当前模型'}支持图片输入`}
        />
      </div>
    </div>
  );
}
