// 「技能」页面：查看智能体已沉淀 / 拉取的技能（Skill），只读 + 删除。
//
// 两级展示：
//   一级 —— 技能列表总览（name + 一句话描述 + 删除），只读 SKILL.md 的 frontmatter；
//   二级 —— 查看走模态框，正文按需拉取（get_skill）。
// 写一个正确、可用的 SKILL.md 是有门槛的，普通用户不一定写得好，因此不开放人工创建/编辑入口；
// 技能由智能体在对话中按需沉淀，或按用户给的链接拉取。用户在此只读查看与删除。

import { useCallback, useEffect, useState } from 'react';
import { PageContainer, Button, Modal, ConfirmDialog } from '../../components/ui';
import { listSkills, getSkill, deleteSkill } from '../../lib/skillApi';
import type { Skill, SkillDetail } from '../../types/skill';
import styles from './SkillPage.module.css';

export function SkillPage() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const [viewing, setViewing] = useState<SkillDetail | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Skill | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError('');
    try {
      setSkills(await listSkills());
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

  const openView = async (skill: Skill) => {
    setError('');
    try {
      setViewing(await getSkill(skill.name));
    } catch (e) {
      setError(String(e));
    }
  };

  const doDelete = async () => {
    if (!confirmDelete) return;
    const name = confirmDelete.name;
    setConfirmDelete(null);
    await deleteSkill(name);
    await refresh();
  };

  return (
    <PageContainer
      title="技能"
      description="可复用的流程知识包 —— 由智能体在对话中按需沉淀，或按你给的链接拉取；你在此查看与删除。"
    >
      <div className={styles.toolbar}>
        <span className={styles.summary}>
          {loading && !loaded ? '加载中…' : `${skills.length} 个技能`}
        </span>
      </div>

      {error && <p className={styles.error}>{error}</p>}

      {loaded && skills.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>还没有技能</p>
          <p className={styles.emptyHint}>在对话里让 Avalon 沉淀技能，或给它一个链接拉取；之后这里就能看到。</p>
        </div>
      ) : (
        <div className={styles.list}>
          {skills.map((skill) => (
            <div key={skill.name} className={styles.row}>
              <button type="button" className={styles.rowMain} onClick={() => openView(skill)}>
                <span className={styles.rowName}>{skill.name}</span>
                {skill.description && (
                  <span className={styles.rowDesc}>{skill.description}</span>
                )}
              </button>
              <div className={styles.rowActions}>
                <Button variant="danger" size="sm" onClick={() => setConfirmDelete(skill)}>
                  删除
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {viewing && <ViewModal detail={viewing} onClose={() => setViewing(null)} />}

      <ConfirmDialog
        open={confirmDelete != null}
        title="删除技能"
        description={`确定删除「${confirmDelete?.name ?? ''}」？其目录与正文将一并删除，不可撤销。`}
        confirmText="删除"
        danger
        onConfirm={doDelete}
        onCancel={() => setConfirmDelete(null)}
      />
    </PageContainer>
  );
}

/** 查看模态框：只读展示 name + description + 正文 + 参考文档列表 */
function ViewModal({ detail, onClose }: { detail: SkillDetail; onClose: () => void }) {
  return (
    <Modal
      open
      onClose={onClose}
      title={detail.name}
      width={640}
      footer={
        <Button variant="secondary" onClick={onClose}>
          关闭
        </Button>
      }
    >
      {detail.description && <p className={styles.viewDesc}>{detail.description}</p>}
      <p className={styles.viewContent}>{detail.content}</p>
      {detail.references.length > 0 && (
        <p className={styles.viewRefs}>参考文档：{detail.references.join('、')}</p>
      )}
    </Modal>
  );
}
