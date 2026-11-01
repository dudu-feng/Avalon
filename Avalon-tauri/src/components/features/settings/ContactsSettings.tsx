import { useEffect, useState } from 'react';
import { Button, Card, ConfirmDialog, Input, Modal } from '../../ui';
import { deleteContact, listContacts, updateContact } from '../../../lib/contactApi';
import type { Contact } from '../../../types/contact';
import styles from './ContactsSettings.module.css';

const PROFILE_MAX_CHARS = 4000;

/**
 * 渠道通讯录管理：列表、搜索、编辑画像、删除。
 *
 * 通讯录只由智能体在对话中主动 record_user / update_user 维护，
 * 这里不提供手工新增，避免记进从未和智能体说过话的人。
 */
export function ContactsSettings() {
  const [users, setUsers] = useState<Contact[]>([]);
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

  const searching = Boolean(query.trim());

  return (
    <Card
      eyebrow="渠道"
      title="通讯录"
      description="只记录智能体在对话中主动登记的用户，不自动拉取群成员。每条只有 id、显示名和画像；用户说过的话请到会话历史里查。"
    >
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
