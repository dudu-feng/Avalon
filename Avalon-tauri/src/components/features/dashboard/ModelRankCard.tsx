// 模型用量排行。
//
// 模型名是「名义分类」——交换顺序不改变含义，所以每根条都用同一个主色，
// 绝不按数值深浅上色：那会把条形长度已经表达的信息再用颜色编码一遍，
// 白白烧掉唯一的身份通道。

import { Card, ProgressBar } from '../../ui';
import type { ModelUsage } from '../../../types/dashboard';
import { formatCompact, formatFull } from './dashboardData';
import styles from './ModelRankCard.module.css';

export interface ModelRankCardProps {
  ranking: ModelUsage[];
}

export function ModelRankCard({ ranking }: ModelRankCardProps) {
  const max = ranking[0]?.totalTokens ?? 0;

  return (
    <Card as="section" className={styles.card}>
      <header className={styles.head}>
        <h3 className={styles.title}>模型用量排行</h3>
        <p className={styles.sub}>按 token 消耗降序</p>
      </header>

      {ranking.length === 0 ? (
        <p className={styles.empty}>这段时间还没有模型调用记录</p>
      ) : (
        <ul className={styles.list}>
          {ranking.map((m) => (
            <li key={m.model} className={styles.item}>
              <div className={styles.row}>
                <span className={styles.name} title={m.model}>
                  {m.model}
                </span>
                <span className={styles.value} title={`${formatFull(m.totalTokens)} token`}>
                  {formatCompact(m.totalTokens)}
                </span>
              </div>
              <ProgressBar value={m.totalTokens} max={max || 1} className={styles.bar} />
              <p className={styles.meta}>{m.requests} 次请求</p>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
