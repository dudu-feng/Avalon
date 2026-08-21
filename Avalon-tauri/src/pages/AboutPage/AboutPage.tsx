// 「关于」页面：项目展示——立意、开源链接、作者联系方式与社交账号。
//
// 联系方式（邮箱）由 contact.ts 运行时拼接，防爬虫；社交账号 hover 悬浮二维码。
// 外链用 openUrl 打开系统浏览器 / 邮件客户端。

import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { PageContainer, Card, Badge, Tooltip } from '../../components/ui';
import { CONTACT, LINKS, SOCIAL } from './contact';
import styles from './AboutPage.module.css';

export function AboutPage() {
  const [version, setVersion] = useState('0.1.0');

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const open = (url: string) => {
    openUrl(url).catch(() => {});
  };

  return (
    <PageContainer
      title="Avalon Agent"
      description="打造属于你的理想乡 · make your own Avalon"
    >
      <Card eyebrow="Links" title="开源与作者">
        <div className={styles.links}>
          <button type="button" className={styles.link} onClick={() => open(LINKS.project)}>
            <span className={styles.linkLabel}>项目 GitHub</span>
            <span className={styles.linkValue}>{LINKS.project}</span>
          </button>
          <button type="button" className={styles.link} onClick={() => open(LINKS.author)}>
            <span className={styles.linkLabel}>作者 GitHub</span>
            <span className={styles.linkValue}>{LINKS.author}</span>
          </button>
        </div>
      </Card>

      <Card eyebrow="Contact" title="联系方式">
        <div className={styles.links}>
          <button type="button" className={styles.link} onClick={() => open('mailto:' + CONTACT.qqMail)}>
            <span className={styles.linkLabel}>QQ 邮箱</span>
            <span className={styles.linkValue}>{CONTACT.qqMail}</span>
          </button>
          <button type="button" className={styles.link} onClick={() => open('mailto:' + CONTACT.email)}>
            <span className={styles.linkLabel}>Email</span>
            <span className={styles.linkValue}>{CONTACT.email}</span>
          </button>
        </div>
      </Card>

      <Card eyebrow="Social" title="社交账号">
        <div className={styles.socials}>
          {SOCIAL.map((s) => (
            <Tooltip
              key={s.name}
              side="top"
              delayMs={120}
              label={
                <div className={styles.qr}>
                  <img src={s.qr} alt={`${s.name} 二维码`} />
                  <span>{s.name} 扫码关注</span>
                </div>
              }
            >
              <span className={styles.socialName}>@{s.account}</span>
            </Tooltip>
          ))}
        </div>
      </Card>

      <Card eyebrow="About" title="关于本项目">
        <div className={styles.row}>
          <Badge variant="filled">v{version}</Badge>
          <Badge variant="muted">Rust</Badge>
          <Badge variant="muted">Tauri 2</Badge>
          <Badge variant="muted">React 19</Badge>
          <Badge variant="outline">TypeScript</Badge>
        </div>
      </Card>
    </PageContainer>
  );
}
