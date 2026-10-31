import type { ChangeEvent } from 'react';
import { Card, Dropdown, Input, Switch } from '../../ui';
import type { ModelConfig, ObserveMode, VisionConfig } from '../../../types/config';
import styles from './VisionSettings.module.css';

export interface VisionSettingsProps {
  config: VisionConfig;
  models: ModelConfig[];
  activeModel: string;
  onChange: (next: VisionConfig) => void;
}

/** 数值字段白名单：VisionConfig 里还有字符串/布尔字段，不能直接 keyof */
type NumberField =
  | 'max_edge'
  | 'max_bytes'
  | 'jpeg_quality'
  | 'timeout_secs'
  | 'concurrency'
  | 'cache_ttl_days';

const OBSERVE_MODES = [
  { value: 'always', label: 'always（首次观察并缓存，历史可复用）' },
  { value: 'lazy', label: 'lazy（需要文本观察时才生成）' },
];

const DETAIL_OPTIONS = [
  { value: '', label: '不传（使用服务商默认）' },
  { value: 'auto', label: 'auto（自动）' },
  { value: 'low', label: 'low（更省 token）' },
  { value: 'high', label: 'high（更多细节）' },
];

/** 视觉模块设置：图片输入开关、模型选择、预处理参数、外发确认 */
export function VisionSettings({
  config,
  models,
  activeModel,
  onChange,
}: VisionSettingsProps) {
  function patch(p: Partial<VisionConfig>) {
    onChange({ ...config, ...p });
  }

  function setNumber(field: NumberField, value: number) {
    const next: VisionConfig = { ...config };
    next[field] = value;
    onChange(next);
  }

  function onNumber(field: NumberField) {
    return (e: ChangeEvent<HTMLInputElement>) => {
      const n = e.currentTarget.valueAsNumber;
      if (Number.isFinite(n)) setNumber(field, n);
    };
  }

  // 视觉模型下拉用 models[].name（切换锚点），不是 modelname。
  // 配置里可能残留已删除的模型名，额外补一项，避免下拉框静默显示成占位符
  const modelOptions = [
    { value: '', label: '跟随活跃模型（要求它支持图片输入）' },
    ...models.map((m) => ({
      value: m.name,
      label: [
        m.name,
        m.name === activeModel ? '当前活跃' : '',
        m.supports_vision ? '' : '未标记支持图片',
      ]
        .filter(Boolean)
        .join(' · '),
    })),
  ];
  if (config.model.trim() && !models.some((m) => m.name === config.model.trim())) {
    modelOptions.push({
      value: config.model.trim(),
      label: `${config.model.trim()}（不在模型列表中）`,
    });
  }

  const selected = config.model.trim()
    ? models.find((m) => m.name === config.model.trim())
    : models.find((m) => m.name === activeModel);

  let problem: string | null = null;
  if (config.enabled) {
    if (config.model.trim() && !selected) {
      problem = `视觉模型「${config.model.trim()}」不在模型列表中，图片入口不会启用。`;
    } else if (!selected) {
      problem = '当前没有活跃模型，图片入口不会启用。';
    } else if (!selected.supports_vision) {
      problem = `模型「${selected.name}」未标记 supports_vision，图片调用可能失败。`;
    }
  }

  return (
    <>
      <Card
        eyebrow="视觉输入"
        title="图片理解"
        description="开启后聊天页出现图片入口。图片会先在本机缩放、重编码并去除 EXIF，再交给视觉模型。"
      >
        <div className={styles.switchRow}>
          <div className={styles.switchText}>
            <span className={styles.switchLabel}>启用图片输入 enabled</span>
            <span className={styles.switchHint}>
              关闭后聊天页隐藏图片入口；已保存的历史附件仍可查看。
            </span>
          </div>
          <Switch
            checked={config.enabled}
            onChange={(checked) => patch({ enabled: checked })}
            aria-label="启用图片输入"
          />
        </div>

        <div className={styles.grid}>
          <div className={styles.wide}>
            <Dropdown
              label="视觉模型 model"
              options={modelOptions}
              value={config.model.trim()}
              onChange={(value) => patch({ model: value })}
              placeholder="跟随活跃模型"
            />
          </div>
          <Dropdown
            label="观察策略 observe_mode"
            options={OBSERVE_MODES}
            value={config.observe_mode}
            onChange={(value) => patch({ observe_mode: value as ObserveMode })}
          />
          <Dropdown
            label="图片精度 detail"
            options={DETAIL_OPTIONS}
            value={config.detail}
            onChange={(value) => patch({ detail: value })}
          />
          <Input
            label="最长边 max_edge（像素）"
            type="number"
            min="256"
            max="8192"
            step="1"
            value={config.max_edge}
            onChange={onNumber('max_edge')}
          />
          <Input
            label="编码上限 max_bytes（字节）"
            type="number"
            min="65536"
            max="33554432"
            step="1048576"
            value={config.max_bytes}
            onChange={onNumber('max_bytes')}
          />
          <Input
            label="JPEG 质量 jpeg_quality（40–95）"
            type="number"
            min="40"
            max="95"
            step="1"
            value={config.jpeg_quality}
            onChange={onNumber('jpeg_quality')}
          />
          <Input
            label="超时秒数 timeout_secs"
            type="number"
            min="5"
            step="1"
            value={config.timeout_secs}
            onChange={onNumber('timeout_secs')}
          />
          <Input
            label="并发上限 concurrency（1–8）"
            type="number"
            min="1"
            max="8"
            step="1"
            value={config.concurrency}
            onChange={onNumber('concurrency')}
          />
          <Input
            label="缓存保留天数 cache_ttl_days（0 = 不清理）"
            type="number"
            min="0"
            step="1"
            value={config.cache_ttl_days}
            onChange={onNumber('cache_ttl_days')}
          />
        </div>

        {problem && <p className={styles.warn}>{problem}</p>}

        <p className={styles.hint}>
          图片在发送前会统一缩放到最长边 {config.max_edge}px，并在编码超过{' '}
          {(config.max_bytes / 1024 / 1024).toFixed(1)} MiB 时自动降低质量。
          EXIF、GPS 与相机信息不会外发。
        </p>
        <p className={styles.hint}>
          {config.observe_mode === 'always'
            ? 'always 会为每张新图额外生成一次观察文本，供历史轮次和纯文本模型复用；同一张图按内容哈希缓存，只调用一次。'
            : 'lazy 只在需要文本观察时才调用视觉模型，首轮延迟更低，但历史里可能没有图片描述。'}
          缓存与缩略图位于 file_root/cache，可随时删除后重建。
        </p>
        <p className={styles.hint}>
          本地 OCR 快路与缓存自动清理属于 P2，当前配置会保存但尚未接入执行。
        </p>
      </Card>

      <Card
        eyebrow="隐私"
        title="图片外发确认"
        description="图片会离开本机发送给视觉模型服务商。首次向某个模型发送前确认一次，避免误发敏感图片。"
      >
        <div className={styles.switchRow}>
          <div className={styles.switchText}>
            <span className={styles.switchLabel}>首次发送前确认 confirm_before_send</span>
            <span className={styles.switchHint}>
              确认记录按视觉模型分别保存；换模型后会再次询问。
            </span>
          </div>
          <Switch
            checked={config.confirm_before_send}
            onChange={(checked) => patch({ confirm_before_send: checked })}
            aria-label="首次发送图片前确认"
          />
        </div>
        <p className={styles.hint}>
          关闭后图片会在不提示的情况下直接发送给模型服务商。仅在你信任该服务商、
          且确认待发送图片不含敏感信息时关闭。
        </p>
      </Card>
    </>
  );
}
