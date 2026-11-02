import { useEffect, useState } from 'react';
import { Button, Card, ConfirmDialog, Input, Modal } from '../../ui';
import { deleteContact, listContacts, updateContact, updateOwner } from '../../../lib/contactApi';
import type { Contact, OwnerContact } from '../../../types/contact';
import styles from './ContactsSettings.module.css';

const PROFILE_MAX_CHARS = 4000;

/** 主人区固定展示的两个渠道（顺序稳定，不随是否有记录变化） */
const OWNER_CHANNELS = [
  { key: 'feishu', label: '飞书' },
  { key: 'qq', label: 'QQ' },
] as const;

/**
 * 渠道通讯录管理：主人展示 / 更正，用户列表、搜索、编辑画像、删除。
 *
 * 普通用户只由智能体在对话中主动 record_user / update_user 维护，
 * 这里不提供手工新增，避免记进从未和智能体说过话的人。
 * 主人由渠道在首次私聊时自动记录，这里只做展示和纠正。
 */
export function ContactsSettings() {
  const [users, setUsers] = useState<Contact[]>([]);
  const [owners, setOwners] = useState<Record<string, OwnerContact>>({});
  const [total, setTotal] = useState(0);
  const [query, setQuery] = useState('');
  const [reload, setReload] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [editing, setEditing] = useState<Contact | null>(null);
  const [draftName, setDraftName] = useState('');
  const [draftProfile, setDraftProfile] = useState('');
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<Contact | null>(null);

  const [editingOwner, setEditingOwner] = useState<{ key: string; label: string } | null>(null);
  const [draftOwnerId, setDraftOwnerId] = useState('');
  const [draftOwnerName, setDraftOwnerName] = useState('');
  const [savingOwner, setSavingOwner] = useState(false);

  useEffect(() => {
    let alive = true;
    const timer = window.setTimeout(
      async () => {
        setLoading(true);
        setError(null);
        try {
          const res = await listContacts(query);
          if (!alive) return;
          setUsers(res.users);
          setOwners(res.owners ?? {});
          setTotal(res.total);
        } catch (e) {
          if (alive) setError(String(e));
        } finally {
          if (alive) setLoading(false);
        }
      },
      query.trim() ? 250 : 0,
    );
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [query, reload]);

  function openEdit(contact: Contact) {
    setEditing(contact);
    setDraftName(contact.name);
    setDraftProfile(contact.profile);
    setError(null);
    setNotice(null);
  }

  async function handleSave() {
    if (!editing) return;
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const updated = await updateContact(editing.id, draftName.trim(), draftProfile);
      setUsers((list) => list.map((u) => (u.id === updated.id ? updated : u)));
      setEditing(null);
      setNotice(`已保存：${updated.name}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!deleting) return;
    const target = deleting;
    setDeleting(null);
    setError(null);
    setNotice(null);
    try {
      await deleteContact(target.id);
      setNotice(`已删除：${target.name}`);
      setReload((n) => n + 1);
    } catch (e) {
      setError(String(e));
    }
  }

  function openOwnerEdit(key: string, label: string, owner?: OwnerContact) {
    setEditingOwner({ key, label });
    // 展示时带渠道前缀，编辑框里只放裸 id，避免用户手动前缀写错
    setDraftOwnerId(owner ? owner.id.replace(/^(feishu|qq):/, '') : '');
    setDraftOwnerName(owner?.name ?? '');
    setError(null);
    setNotice(null);
  }

  async function handleSaveOwner() {
    if (!editingOwner || !draftOwnerId.trim()) return;
    setSavingOwner(true);
    setError(null);
    setNotice(null);
    try {
      const updated = await updateOwner(editingOwner.key, draftOwnerId.trim(), draftOwnerName);
      setOwners((map) => ({ ...map, [editingOwner.key]: updated }));
      setNotice(`已保存${editingOwner.label}主人`);
      setEditingOwner(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingOwner(false);
    }
  }

  const searching = Boolean(query.trim());

  return (
    <Card
      eyebrow="渠道"
      title="通讯录"
      description="主人由渠道在首次私聊时自动记录；普通用户由智能体在对话中主动登记，不自动拉取群成员。主人只有 id 与昵称（画像归灵魂模块），用户只有 id、显示名和画像。"
    >
      <div className={styles.owners}>
        <div className={styles.ownerHead}>
          <span className={styles.ownerTitle}>主人</span>
          <span className={styles.ownerBadge}>系统自动记录</span>
        </div>
        {OWNER_CHANNELS.map(({ key, label }) => {
          const owner = owners[key];
          return (
            <div key={key} className={styles.ownerRow}>
              <span className={styles.ownerLabel}>{label}</span>
              <div className={styles.ownerIdentity}>
                {owner ? (
                  <>
                    <span className={styles.name}>{owner.name || '未获取昵称'}</span>
                    <span className={styles.id}>{owner.id}</span>
                  </>
                ) : (
                  <span className={styles.ownerEmpty}>未记录 · 首次私聊机器人后自动记录</span>
                )}
              </div>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => openOwnerEdit(key, label, owner)}
              >
                {owner ? '更正' : '补填'}
              </Button>
            </div>
          );
        })}
        <p className={styles.hint}>
          主人只存 id 和昵称，没有用户画像；主人画像由灵魂模块维护。普通用户由智能体在对话中主动记录到下方列表。
        </p>
      </div>

      <div className={styles.toolbar}>
        <Input
          className={styles.search}
          placeholder="按 id / 显示名 / 画像关键词搜索"
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
        />
        <Button variant="secondary" onClick={() => setReload((n) => n + 1)} disabled={loading}>
          {loading ? '刷新中…' : '刷新'}
        </Button>
      </div>

      <p className={styles.meta}>
        共 {total} 人
        {searching && `，匹配 ${users.length} 人`}
        {loading && users.length > 0 && ' · 正在更新…'}
      </p>

      {error && <p className={styles.error}>{error}</p>}
      {notice && <p className={styles.notice}>{notice}</p>}

      {users.length === 0 && !loading ? (
        <p className={styles.hint}>
          {searching
            ? '没有匹配的用户。'
            : '通讯录为空。智能体在对话里遇到值得长期记住的人时，会主动记录到这里。'}
        </p>
      ) : (
        <ul className={styles.list}>
          {users.map((contact) => (
            <li key={contact.id} className={styles.item}>
              <div className={styles.itemHead}>
                <div className={styles.identity}>
                  <span className={styles.name}>{contact.name}</span>
                  <span className={styles.id}>{contact.id}</span>
                </div>
                <div className={styles.actions}>
                  <Button variant="secondary" size="sm" onClick={() => openEdit(contact)}>
                    编辑
                  </Button>
                  <Button variant="danger" size="sm" onClick={() => setDeleting(contact)}>
                    删除
                  </Button>
                </div>
              </div>
              <p className={styles.profile}>{contact.profile || '暂无画像'}</p>
            </li>
          ))}
        </ul>
      )}

      <Modal
        open={Boolean(editing)}
        onClose={() => !saving && setEditing(null)}
        title={editing ? `编辑：${editing.name}` : '编辑通讯录'}
        description={editing?.id}
        width={560}
        footer={
          <>
            <Button variant="secondary" onClick={() => setEditing(null)} disabled={saving}>
              取消
            </Button>
            <Button onClick={() => void handleSave()} disabled={saving || !draftName.trim()}>
              {saving ? '保存中…' : '保存'}
            </Button>
          </>
        }
      >
        <Input
          label="显示名 name"
          value={draftName}
          maxLength={100}
          onChange={(e) => setDraftName(e.currentTarget.value)}
        />
        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor="contact-profile">
            用户画像 profile（整体替换，{PROFILE_MAX_CHARS} 字以内）
          </label>
          <textarea
            id="contact-profile"
            className={styles.textarea}
            value={draftProfile}
            maxLength={PROFILE_MAX_CHARS}
            placeholder="身份、关注点、沟通偏好、与主人的关系、注意事项……"
            onChange={(e) => setDraftProfile(e.currentTarget.value)}
          />
        </div>
      </Modal>

      <Modal
        open={Boolean(editingOwner)}
        onClose={() => !savingOwner && setEditingOwner(null)}
        title={editingOwner ? `更正${editingOwner.label}主人` : '更正主人'}
        description="通常由首次私聊自动记录；这里用于纠正误记录。保存时自动补渠道前缀。"
        width={520}
        footer={
          <>
            <Button
              variant="secondary"
              onClick={() => setEditingOwner(null)}
              disabled={savingOwner}
            >
              取消
            </Button>
            <Button
              onClick={() => void handleSaveOwner()}
              disabled={savingOwner || !draftOwnerId.trim()}
            >
              {savingOwner ? '保存中…' : '保存'}
            </Button>
          </>
        }
      >
        <Input
          label="主人 id"
          value={draftOwnerId}
          placeholder="ou_xxx / openid"
          onChange={(e) => setDraftOwnerId(e.currentTarget.value)}
        />
        <Input
          label="昵称（可留空）"
          value={draftOwnerName}
          maxLength={100}
          onChange={(e) => setDraftOwnerName(e.currentTarget.value)}
        />
      </Modal>

      <ConfirmDialog
        open={Boolean(deleting)}
        title="删除通讯录条目"
        description={
          deleting
            ? `确定删除「${deleting.name}」（${deleting.id}）？画像会一并删除，且不可恢复。`
            : undefined
        }
        confirmText="删除"
        danger
        onConfirm={() => void handleDelete()}
        onCancel={() => setDeleting(null)}
      />
    </Card>
  );
}
