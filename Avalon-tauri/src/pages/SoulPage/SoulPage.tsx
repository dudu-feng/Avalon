// 「灵魂」页面：查看 / 编辑智能体灵魂与用户画像的条目注册表。
//
// 两级展示：
//   一级 —— 条目列表总览（title + 编辑/删除 + 注入 switch），只读 registry.json 索引，不含正文；
//   二级 —— 查看 / 编辑均走模态框，正文存独立 md，打开时按需拉取（get_soul_entry）。
// system 基本设定不在此展示（后端已过滤），仅作默认系统注入。
// 灵魂（自我设定）与用户画像统一为「条目」管理：seed 可编辑不可删、user 可增删改。
// agent 在对话中自主维护（list/get/create/update/delete/set_enabled 工具），这里提供人工入口。

import { useCallback, useEffect, useState } from 'react';
import {
  PageContainer,
  Tabs,
  Button,
  Badge,
  Modal,
  Input,
  ConfirmDialog,
  Switch,
} from '../../components/ui';
import {
  listSoulEntries,
  getSoulEntry,
  createSoulEntry,
  updateSoulEntry,
  deleteSoulEntry,
  setSoulEntryEnabled,
} from '../../lib/soulApi';
import type { EntryCategory, EntryKind, SoulEntry, SoulEntryDetail } from '../../types/soul';
import styles from './SoulPage.module.css';

type Tab = EntryCategory;

const TABS = [
  { value: 'soul', label: '灵魂' },
  { value: 'profile', label: '用户画像' },
];

const KIND_LABEL: Record<EntryKind, string> = {
  system: '系统',
  seed: '初始',
  user: '自定义',
};

const KIND_VARIANT: Record<EntryKind, 'muted' | 'outline' | 'filled'> = {
  system: 'muted',
  seed: 'outline',
  user: 'filled',
};

export function SoulPage() {
  const [tab, setTab] = useState<Tab>('soul');
  const [entries, setEntries] = useState<SoulEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const [viewing, setViewing] = useState<SoulEntryDetail | null>(null);
  const [editing, setEditing] = useState<SoulEntryDetail | null>(null);
  const [creating, setCreating] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<SoulEntry | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      setEntries(await listSoulEntries());
      setLoaded(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const visible = entries.filter((e) => e.category === tab);

  const openView = async (entry: SoulEntry) => {
    setError('');
    try {
      setViewing(await getSoulEntry(entry.id));
    } catch (e) {
      setError(String(e));
    }
  };

  const openEdit = async (entry: SoulEntry) => {
    setError('');
    // 已在查看同一条目时直接转入编辑，避免重复拉取正文
    if (viewing && viewing.id === entry.id) {
      setEditing(viewing);
      setViewing(null);
      return;
    }
    try {
      setEditing(await getSoulEntry(entry.id));
    } catch (e) {
      setError(String(e));
    }
  };

  const openCreate = () => {
    setError('');
    setCreating(true);
  };

  const closeAll = () => {
    setViewing(null);
    setEditing(null);
    setCreating(false);
  };

  const toggleEnabled = async (entry: SoulEntry) => {
    setError('');
    try {
      await setSoulEntryEnabled(entry.id, !entry.enabled);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const submitForm = async (title: string, content: string) => {
    if (editing) {
      await updateSoulEntry(editing.id, title, content);
    } else {
      await createSoulEntry(tab, title, content);
    }
    closeAll();
    await refresh();
  };

  const doDelete = async () => {
    if (!confirmDelete) return;
    const id = confirmDelete.id;
    setConfirmDelete(null);
    await deleteSoulEntry(id);
    await refresh();
  };

  return (
    <PageContainer
      title="灵魂"
      description="智能体的自我设定与关于你的长期记忆 —— Avalon 会在对话中自主完善，你也能随时查看与编辑。"
    >
      <Tabs options={TABS} value={tab} onChange={(v) => setTab(v as Tab)} />

      <div className={styles.toolbar}>
        <span className={styles.summary}>
          {loading && !loaded ? '加载中…' : `${visible.length} 条`}
        </span>
        <Button variant="primary" size="sm" onClick={openCreate}>
          ＋ 新增条目
        </Button>
      </div>

      {error && <p className={styles.error}>{error}</p>}

      {loaded && visible.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>还没有条目</p>
          <p className={styles.emptyHint}>点右上角「新增条目」，或让 Avalon 在对话中为你积累。</p>
        </div>
      ) : (
        <div className={styles.list}>
          {visible.map((entry) => (
            <div key={entry.id} className={styles.row}>
              <button type="button" className={styles.rowMain} onClick={() => openView(entry)}>
                <span className={styles.rowTitle}>{entry.title}</span>
                <Badge variant={KIND_VARIANT[entry.kind]}>{KIND_LABEL[entry.kind]}</Badge>
              </button>
              <div className={styles.rowActions}>
                <Button variant="secondary" size="sm" onClick={() => openEdit(entry)}>
                  编辑
                </Button>
                {entry.kind === 'user' && (
                  <Button variant="danger" size="sm" onClick={() => setConfirmDelete(entry)}>
                    删除
                  </Button>
                )}
                <Switch
                  checked={entry.enabled}
                  onChange={() => toggleEnabled(entry)}
                  aria-label={`${entry.enabled ? '停用' : '启用'}「${entry.title}」的注入`}
                />
              </div>
            </div>
          ))}
        </div>
      )}

      {viewing && (
        <ViewModal
          detail={viewing}
          onEdit={() => {
            setEditing(viewing);
            setViewing(null);
          }}
          onClose={() => setViewing(null)}
        />
      )}

      {(creating || editing) && (
        <EntryFormModal
          key={editing ? editing.id : 'create'}
          title={editing ? '编辑条目' : '新增条目'}
          initial={editing ?? undefined}
          onSubmit={submitForm}
          onClose={closeAll}
        />
      )}

      <ConfirmDialog
        open={confirmDelete != null}
        title="删除条目"
        description={`确定删除「${confirmDelete?.title ?? ''}」？此操作不可撤销。`}
        confirmText="删除"
        danger
        onConfirm={doDelete}
        onCancel={() => setConfirmDelete(null)}
      />
    </PageContainer>
  );
}

/** 查看模态框：只读展示标题 + 正文，底部可转入编辑 */
function ViewModal({
  detail,
  onEdit,
  onClose,
}: {
  detail: SoulEntryDetail;
  onEdit: () => void;
  onClose: () => void;
}) {
  return (
    <Modal
      open
      onClose={onClose}
      title={detail.title}
      width={640}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            关闭
          </Button>
          <Button variant="primary" onClick={onEdit}>
            编辑
          </Button>
        </>
      }
    >
      <p className={styles.viewContent}>{detail.content}</p>
    </Modal>
  );
}

/** 新增/编辑共用的条目表单：initial 为空则新增，否则预填编辑 */
function EntryFormModal({
  title,
  initial,
  onSubmit,
  onClose,
}: {
  title: string;
  initial?: SoulEntryDetail;
  onSubmit: (title: string, content: string) => Promise<void>;
  onClose: () => void;
}) {
  const [titleValue, setTitleValue] = useState(initial?.title ?? '');
  const [contentValue, setContentValue] = useState(initial?.content ?? '');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');

  const submit = async () => {
    setError('');
    if (!titleValue.trim()) {
      setError('请填写条目标题');
      return;
    }
    if (!contentValue.trim()) {
      setError('请填写条目内容');
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(titleValue.trim(), contentValue.trim());
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      open
      onClose={onClose}
      title={title}
      width={520}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" onClick={submit} disabled={submitting}>
            {submitting ? '保存中…' : '保存'}
          </Button>
        </>
      }
    >
      <div className={styles.form}>
        <Input
          label="条目标题"
          value={titleValue}
          placeholder="例如：偏好与习惯"
          onChange={(e) => setTitleValue(e.currentTarget.value)}
        />
        <label className={styles.fieldLabel} htmlFor="entry-content">
          条目内容
        </label>
        <textarea
          id="entry-content"
          className={styles.textarea}
          value={contentValue}
          rows={8}
          placeholder="描述这条内容"
          onChange={(e) => setContentValue(e.currentTarget.value)}
        />
        {error && <p className={styles.error}>{error}</p>}
      </div>
    </Modal>
  );
}
