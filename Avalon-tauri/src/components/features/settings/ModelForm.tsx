import { Input } from '../../ui';
import type { ModelConfig } from '../../../types/config';
import styles from './ModelForm.module.css';

export interface ModelFormProps {
  model: ModelConfig;
  onChange: (next: ModelConfig) => void;
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
    </div>
  );
}
