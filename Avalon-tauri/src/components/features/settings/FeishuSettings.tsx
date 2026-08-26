import { useEffect, useState } from 'react';
import { Badge, Button, Card, Dropdown, Input } from '../../ui';
import {
  getFeishuStatus,
  startFeishu,
  stopFeishu,
  testFeishuConnection,
} from '../../../lib/channelApi';
import type { BadgeVariant } from '../../../types';
import type { ChannelStatus, FeishuConfig, FeishuSessionMode } from '../../../types/config';
import styles from './FeishuSettings.module.css';

export interface FeishuSettingsProps {
  config: FeishuConfig;
  onChange: (next: FeishuConfig) => void;
  /** 有未保存改动。后端读的是已落盘的配置，此时不能启停或测试 */
  dirty: boolean;
}

const ENABLED_OPTIONS = [
  { value: 'true', label: '启用（随应用自动连接）' },
  { value: 'false', label: '停用' },
];
const MENTION_OPTIONS = [
  { value: 'true', label: '必须 @ 机器人' },
  { value: 'false', label: '群内任何消息都响应' },
];
const SESSION_MODES = [
  { value: 'isolated', label: 'isolated（每个聊天独立上下文）' },
  { value: 'unified', label: 'unified（全部汇入同一会话）' },
];

/** 状态轮询间隔。连接中／重连中时状态变化较快，3 秒足够跟上又不至于太吵 */
const POLL_MS = 3000;

/** 渠道状态 → 徽标文案与样式 */
function describe(status: ChannelStatus | null): { label: string; variant: BadgeVariant } {
  switch (status?.state) {
    case 'running':
      return { label: '运行中', variant: 'filled' };
    case 'connecting':
      return { label: '连接中…', variant: 'outline' };
    case 'reconnecting':
      return { label: '重连中…', variant: 'outline' };
    case 'stopped':
      return { label: '已停止', variant: 'muted' };
    case 'error':
      return { label: '连接错误', variant: 'outline' };
    case 'disabled':
      return { label: '未启用', variant: 'muted' };
    default:
      return { label: '查询中…', variant: 'muted' };
  }
}

/** 长连接处于活动态（含重连），此时该给的是「停止」而不是「启动」 */
function isActive(status: ChannelStatus | null): boolean {
  return (
    status?.state === 'running' ||
    status?.state === 'connecting' ||
    status?.state === 'reconnecting'
  );
}

/** 逗号／空白分隔的 open_id 列表 → 去空数组 */
function parseList(text: string): string[] {
  return text
    .split(/[,，\s]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** 飞书渠道配置与连接控制 */
export function FeishuSettings({ config, onChange, dirty }: FeishuSettingsProps) {
  const [status, setStatus] = useState<ChannelStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  // 白名单在本地以文本编辑，失焦时才解析回数组 ——
  // 否则每敲一个逗号就会被 split 掉，没法接着输入下一个 id
  const [allowText, setAllowText] = useState(() => config.allow_users.join(', '));

  // 本组件只在「渠道」标签页激活时挂载，离开即卸载，轮询随之停止
  useEffect(() => {
    let alive = true;
    const poll = () => {
      getFeishuStatus()
        .then((s) => alive && setStatus(s))
        .catch(() => alive && setStatus(null));
    };
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  // 外部重新加载配置时同步回文本框（也顺带把失焦解析的结果规范化显示）
  useEffect(() => {
    setAllowText(config.allow_users.join(', '));
  }, [config.allow_users]);

  function patch<K extends keyof FeishuConfig>(key: K, value: FeishuConfig[K]) {
    onChange({ ...config, [key]: value });
  }

  async function run(action: () => Promise<void>, okText: string) {
    setBusy(true);
    setNotice(null);
    try {
      await action();
      setNotice(okText);
      setStatus(await getFeishuStatus());
    } catch (e) {
      setNotice(`${e}`);
    } finally {
      setBusy(false);
    }
  }

  const { label, variant } = describe(status);
  const active = isActive(status);
  // 后端读的是已落盘的配置，草稿状态下启停只会用到旧值，徒增困惑
  const blocked = busy || dirty;

  return (
    <>
      <Card
        eyebrow="飞书"
        title="连接状态"
        description="长连接由企业自建应用的 app_id / app_secret 驱动，不需要公网回调地址。"
      >
        <div className={styles.statusRow}>
          <Badge variant={variant}>{label}</Badge>
          <div className={styles.statusActions}>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void run(testFeishuConnection, '凭证验证通过')}
              disabled={blocked}
            >
              测试连接
            </Button>
            {active ? (
              <>
                {/* 配置改动不会热生效，重连是唯一的应用途径（内部先停后启） */}
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void run(startFeishu, '已重新连接')}
                  disabled={blocked}
                >
                  重新连接
                </Button>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void run(stopFeishu, '已停止')}
                  disabled={busy}
                >
                  停止
                </Button>
              </>
            ) : (
              <Button
                variant="primary"
                size="sm"
                onClick={() => void run(startFeishu, '已启动')}
                disabled={blocked}
              >
                启动
              </Button>
            )}
          </div>
        </div>

        {status?.state === 'error' && <p className={styles.error}>{status.message}</p>}
        {notice && <p className={styles.notice}>{notice}</p>}
        {dirty && (
          <p className={styles.hint}>
            有未保存的改动。启动与测试读取的是已保存的配置，请先点右上角「保存配置」。
          </p>
        )}
        {!dirty && active && (
          <p className={styles.hint}>配置改动不会热生效，保存后需点「重新连接」才会应用。</p>
        )}
        <p className={styles.hint}>
          长连接为集群模式且不广播消息：同一个飞书应用同时只应有一台机器在线，否则消息会被随机分走。
        </p>
      </Card>

      <Card
        eyebrow="凭证"
        title="应用信息"
        description="飞书开放平台「凭证与基础信息」页。仅支持企业自建应用，商店应用用不了长连接。"
      >
        <div className={styles.grid}>
          <Dropdown
            label="启用状态 enabled"
            options={ENABLED_OPTIONS}
            value={String(config.enabled)}
            onChange={(v) => patch('enabled', v === 'true')}
          />
          <Input
            label="App ID app_id"
            value={config.app_id}
            placeholder="cli_ 开头"
            onChange={(e) => patch('app_id', e.currentTarget.value)}
          />
          <Input
            label="App Secret app_secret"
            type="password"
            value={config.app_secret}
            placeholder="可用环境变量 AVALON_FEISHU_APP_SECRET 覆盖"
            onChange={(e) => patch('app_secret', e.currentTarget.value)}
          />
          <Input
            label="开放平台域名 domain"
            value={config.domain}
            placeholder="https://open.feishu.cn"
            onChange={(e) => patch('domain', e.currentTarget.value)}
          />
        </div>
      </Card>

      <Card
        eyebrow="行为"
        title="响应与会话"
        description="谁能触发、群里怎么触发，以及各个聊天之间记忆是否隔离。"
      >
        <div className={styles.grid}>
          <Dropdown
            label="群聊触发 group_require_mention"
            options={MENTION_OPTIONS}
            value={String(config.group_require_mention)}
            onChange={(v) => patch('group_require_mention', v === 'true')}
          />
          <Dropdown
            label="会话隔离 session_mode"
            options={SESSION_MODES}
            value={config.session_mode}
            onChange={(v) => patch('session_mode', v as FeishuSessionMode)}
          />
          <Input
            label="用户白名单 allow_users（逗号分隔，留空 = 不限制）"
            value={allowText}
            placeholder="ou_xxx, ou_yyy"
            onChange={(e) => setAllowText(e.currentTarget.value)}
            onBlur={() => patch('allow_users', parseList(allowText))}
          />
        </div>
      </Card>

      <Card
        eyebrow="状态表情"
        title="处理进度标记"
        description="收到消息时给用户那条消息打表情，跑完换成完成态。留空则不打。需要 im:message.reaction:write 权限。"
      >
        <div className={styles.grid}>
          <Input
            label="处理中 processing_reaction"
            value={config.processing_reaction}
            placeholder="OnIt"
            onChange={(e) => patch('processing_reaction', e.currentTarget.value)}
          />
          <Input
            label="已完成 done_reaction"
            value={config.done_reaction}
            placeholder="DONE"
            onChange={(e) => patch('done_reaction', e.currentTarget.value)}
          />
        </div>
      </Card>
    </>
  );
}
