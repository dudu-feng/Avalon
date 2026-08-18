import { useEffect, useState, type ChangeEvent } from 'react';
import { PageContainer, Card, Button, Input, Select, Badge } from '../../components/ui';
import { ModelCard } from '../../components/features/settings';
import { getConfig, saveConfig, rebuildMemoryIndex } from '../../lib/settingsApi';
import type {
  AppConfig,
  ModelConfig,
  EmbeddingMode,
  EmbeddingLoadMode,
  SearchMode,
  VectorBackend,
} from '../../types/config';
import styles from './SettingsPage.module.css';

const EMBEDDING_MODES = [
  { value: 'local', label: 'local（本地模型）' },
  { value: 'api', label: 'api（接口）' },
];
const LOAD_MODES = [
  { value: 'lazy', label: 'lazy（懒加载）' },
  { value: 'eager', label: 'eager（常驻热加载）' },
];
const SEARCH_MODES = [
  { value: 'semantic', label: 'semantic（语义）' },
  { value: 'keyword', label: 'keyword（关键词）' },
  { value: 'hybrid', label: 'hybrid（混合）' },
];
const VECTOR_BACKENDS = [
  { value: 'memory', label: 'memory（自研轻量）' },
  { value: 'sqlite', label: 'sqlite（预留扩展）' },
];

/** 数字输入处理器：仅在解析为有限数时更新，避免清空时被重置为 0 */
function onNumber(updater: (n: number) => void) {
  return (e: ChangeEvent<HTMLInputElement>) => {
    const n = e.currentTarget.valueAsNumber;
    if (Number.isFinite(n)) updater(n);
  };
}

export function SettingsPage() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [dirty, setDirty] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [rebuilding, setRebuilding] = useState(false);
  const [rebuildResult, setRebuildResult] = useState<string | null>(null);

  useEffect(() => {
    getConfig()
      .then(setConfig)
      .catch((e) => setWarnings([`加载配置失败: ${e}`]));
  }, []);

  function update<K extends keyof AppConfig>(key: K, value: AppConfig[K]) {
    setConfig((c) => (c ? { ...c, [key]: value } : c));
    setDirty(true);
    setSaved(false);
  }

  async function handleSave() {
    if (!config) return;
    setSaving(true);
    try {
      const warns = await saveConfig(config);
      setWarnings(warns);
      setDirty(false);
      setSaved(true);
    } catch (e) {
      setWarnings([`保存失败: ${e}`]);
    } finally {
      setSaving(false);
    }
  }

  // ============ 模型列表操作 ============

  function addModel() {
    if (!config) return;
    update('models', [...config.models, { name: '', url: '', key: '', modelname: '' }]);
  }

  function updateModel(index: number, next: ModelConfig) {
    if (!config) return;
    update(
      'models',
      config.models.map((m, i) => (i === index ? next : m)),
    );
  }

  function removeModel(index: number) {
    if (!config) return;
    const target = config.models[index];
    if (!window.confirm(`删除模型「${target.name || '未命名'}」？`)) return;
    const models = config.models.filter((_, i) => i !== index);
    const activeModel =
      target.name === config.active_model ? (models[0]?.name ?? '') : config.active_model;
    setConfig((c) => (c ? { ...c, models, active_model: activeModel } : c));
    setDirty(true);
    setSaved(false);
  }

  function setActive(name: string) {
    if (name === config?.active_model) return;
    update('active_model', name);
  }

  // ============ 维护操作 ============

  async function handleRebuild() {
    if (!window.confirm('确定重建会话向量库？将清空现有索引并重新扫描全部会话。')) return;
    setRebuilding(true);
    setRebuildResult(null);
    try {
      const stats = await rebuildMemoryIndex();
      setRebuildResult(
        `重建完成：归档 ${stats.archived_sessions}、活跃 ${stats.active_sessions}、共 ${stats.total_chunks} 块` +
          (stats.errors.length ? `，${stats.errors.length} 个错误` : ''),
      );
    } catch (e) {
      setRebuildResult(`重建失败: ${e}`);
    } finally {
      setRebuilding(false);
    }
  }

  if (!config) {
    return (
      <PageContainer title="设置" description="加载配置中…">
        <Card variant="sunken" description="正在读取配置…" />
      </PageContainer>
    );
  }

  return (
    <PageContainer title="设置" description="配置应用与模型调用列表，保存后即时生效。">
      {/* 顶部工具行 */}
      <div className={styles.toolbar}>
        <div className={styles.status}>
          {dirty && <Badge variant="outline">未保存</Badge>}
          {saved && !dirty && <Badge variant="muted">已保存</Badge>}
        </div>
        <Button variant="primary" onClick={handleSave} disabled={saving || !dirty}>
          {saving ? '保存中…' : '保存配置'}
        </Button>
      </div>

      {/* 校验警告 */}
      {warnings.length > 0 && (
        <Card variant="sunken" eyebrow="校验结果" title="以下配置项需要关注">
          <ul className={styles.warningList}>
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        </Card>
      )}

      {/* LLM 模型列表 */}
      <Card
        eyebrow="LLM 模型"
        title="模型调用列表"
        description="逐模型独立配置连接参数（url / key / modelname），切换「当前使用」的模型。"
      >
        <div className={styles.modelList}>
          {config.models.map((m, i) => (
            <ModelCard
              key={i}
              model={m}
              isActive={m.name === config.active_model}
              onChange={(next) => updateModel(i, next)}
              onRemove={() => removeModel(i)}
              onSetActive={() => setActive(m.name)}
            />
          ))}
        </div>
        <div className={styles.addRow}>
          <Button variant="secondary" size="sm" onClick={addModel}>
            + 添加模型
          </Button>
        </div>

        <div className={styles.subsection}>
          <h4 className={styles.subheading}>调用参数（全局共享）</h4>
          <div className={styles.grid}>
            <Input
              label="对话温度 chat_temperature"
              type="number"
              step="0.1"
              value={config.llm.chat_temperature}
              onChange={onNumber((n) => update('llm', { ...config.llm, chat_temperature: n }))}
            />
            <Input
              label="JSON 温度 json_temperature"
              type="number"
              step="0.1"
              value={config.llm.json_temperature}
              onChange={onNumber((n) => update('llm', { ...config.llm, json_temperature: n }))}
            />
            <Input
              label="超时秒数 timeout_secs"
              type="number"
              step="1"
              value={config.llm.timeout_secs}
              onChange={onNumber((n) => update('llm', { ...config.llm, timeout_secs: n }))}
            />
          </div>
        </div>
      </Card>

      {/* Embedding */}
      <Card eyebrow="Embedding" title="向量化" description="文本嵌入模型与加载时机。">
        <div className={styles.grid}>
          <Select
            label="模式 mode"
            options={EMBEDDING_MODES}
            value={config.embedding.mode}
            onChange={(v) => update('embedding', { ...config.embedding, mode: v as EmbeddingMode })}
          />
          <Select
            label="加载时机 load_mode"
            options={LOAD_MODES}
            value={config.embedding.load_mode}
            onChange={(v) =>
              update('embedding', { ...config.embedding, load_mode: v as EmbeddingLoadMode })
            }
          />
          <Input
            label="本地模型 local_model"
            value={config.embedding.local_model}
            onChange={(e) =>
              update('embedding', { ...config.embedding, local_model: e.currentTarget.value })
            }
          />
          <Input
            label="设备 device"
            value={config.embedding.device}
            onChange={(e) => update('embedding', { ...config.embedding, device: e.currentTarget.value })}
          />
          <Input
            label="API Key api_key"
            type="password"
            value={config.embedding.api_key}
            onChange={(e) =>
              update('embedding', { ...config.embedding, api_key: e.currentTarget.value })
            }
          />
          <Input
            label="API 模型 api_model"
            value={config.embedding.api_model}
            onChange={(e) =>
              update('embedding', { ...config.embedding, api_model: e.currentTarget.value })
            }
          />
          <Input
            label="API 地址 api_base_url"
            value={config.embedding.api_base_url}
            onChange={(e) =>
              update('embedding', { ...config.embedding, api_base_url: e.currentTarget.value })
            }
          />
        </div>
      </Card>

      {/* 会话记忆 */}
      <Card eyebrow="会话记忆" title="压缩与检索" description="自动压缩阈值、渐进式总结与检索模式。">
        <div className={styles.grid}>
          <Input
            label="压缩阈值 compress_threshold"
            type="number"
            step="1"
            value={config.session_memory.compress_threshold}
            onChange={onNumber((n) =>
              update('session_memory', { ...config.session_memory, compress_threshold: n }),
            )}
          />
          <Input
            label="最大块数 max_chunks"
            type="number"
            step="1"
            value={config.session_memory.max_chunks}
            onChange={onNumber((n) =>
              update('session_memory', { ...config.session_memory, max_chunks: n }),
            )}
          />
          <Input
            label="上下文块数 context_chunks"
            type="number"
            step="1"
            value={config.session_memory.context_chunks}
            onChange={onNumber((n) =>
              update('session_memory', { ...config.session_memory, context_chunks: n }),
            )}
          />
          <Select
            label="检索模式 search_mode"
            options={SEARCH_MODES}
            value={config.session_memory.search_mode}
            onChange={(v) =>
              update('session_memory', { ...config.session_memory, search_mode: v as SearchMode })
            }
          />
        </div>
      </Card>

      {/* Whisper */}
      <Card eyebrow="Whisper" title="语音转写" description="语音识别模型与设备。">
        <div className={styles.grid}>
          <Input
            label="模型 model_name"
            value={config.whisper.model_name}
            onChange={(e) => update('whisper', { ...config.whisper, model_name: e.currentTarget.value })}
          />
          <Input
            label="设备 device"
            value={config.whisper.device}
            onChange={(e) => update('whisper', { ...config.whisper, device: e.currentTarget.value })}
          />
        </div>
      </Card>

      {/* 向量库 */}
      <Card eyebrow="向量库" title="后端存储" description="向量检索的后端实现。">
        <div className={styles.grid}>
          <Select
            label="后端 backend"
            options={VECTOR_BACKENDS}
            value={config.vector.backend}
            onChange={(v) => update('vector', { backend: v as VectorBackend })}
          />
        </div>
      </Card>

      {/* 路径 */}
      <Card eyebrow="路径" title="数据目录" description="共享数据根目录与文件目录，留空则按约定自动推导。">
        <div className={styles.grid}>
          <Input
            label="数据根目录 data_root"
            value={config.paths.data_root}
            onChange={(e) => update('paths', { ...config.paths, data_root: e.currentTarget.value })}
          />
          <Input
            label="文件目录 file_root"
            value={config.paths.file_root}
            onChange={(e) => update('paths', { ...config.paths, file_root: e.currentTarget.value })}
          />
        </div>
      </Card>

      {/* 维护操作 */}
      <Card eyebrow="维护" title="重建向量库" description="清空现有索引，重新扫描全部归档与活跃会话并入库。">
        <div className={styles.maintain}>
          <Button variant="secondary" size="sm" onClick={handleRebuild} disabled={rebuilding}>
            {rebuilding ? '重建中…' : '重建会话向量库'}
          </Button>
          {rebuildResult && <p className={styles.rebuildResult}>{rebuildResult}</p>}
        </div>
      </Card>
    </PageContainer>
  );
}
