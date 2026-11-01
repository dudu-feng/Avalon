import { useEffect, useState } from 'react';
import { Badge, Button, Card, Dropdown, Input } from '../../ui';
import { getQqStatus, startQq, stopQq, testQqConnection } from '../../../lib/channelApi';
import type { BadgeVariant } from '../../../types';
import type {
  ChannelStatus,
  QqBackend,
  QqConfig,
  QqOfficialConfig,
  QqSessionMode,
} from '../../../types/config';
import styles from './QqSettings.module.css';

export interface QqSettingsProps {
  config: QqConfig;
  onChange: (next: QqConfig) => void;
  /** 有未保存改动。后端读的是已落盘的配置，此时不能启停或测试 */
  dirty: boolean;
}

const ENABLED_OPTIONS = [
  { value: 'true', label: '启用（随应用自动连接）' },
  { value: 'false', label: '停用' },
];
const BACKEND_OPTIONS = [
  { value: 'official', label: 'official（QQ 开放平台官方机器人）' },
  { value: 'onebot', label: 'onebot（未实现，仅预留）' },
];
const MENTION_OPTIONS = [
  { value: 'true', label: '必须 @ 机器人' },
  { value: 'false', label: '群内任何消息都响应' },
];
const SESSION_MODES = [
  { value: 'isolated', label: 'isolated（每个群 / 私聊独立上下文）' },
  { value: 'unified', label: 'unified（全部汇入同一会话）' },
];
const SANDBOX_OPTIONS = [
  { value: 'true', label: '沙箱环境（sandbox.api.sgroup.qq.com）' },
  { value: 'false', label: '正式环境（api.bot.qq.com）' },
];
const STREAM_OPTIONS = [
  { value: 'true', label: '单聊启用流式（推荐）' },
  { value: 'false', label: '单聊也走非流式（排障用）' },
];

const POLL_MS = 3000;

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

function isActive(status: ChannelStatus | null): boolean {
  return (
    status?.state === 'running' ||
    status?.state === 'connecting' ||
    status?.state === 'reconnecting'
  );
}

function parseList(text: string): string[] {
  return text
    .split(/[,，\s]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** QQ 渠道配置与连接控制 */
export function QqSettings({ config, onChange, dirty }: QqSettingsProps) {
  const [status, setStatus] = useState<ChannelStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [allowText, setAllowText] = useState(() => config.allow_users.join(', '));

  useEffect(() => {
    let alive = true;
    const poll = () => {
      getQqStatus()
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

  useEffect(() => {
    setAllowText(config.allow_users.join(', '));
  }, [config.allow_users]);

  function patch<K extends keyof QqConfig>(key: K, value: QqConfig[K]) {
    onChange({ ...config, [key]: value });
  }

  function patchOfficial<K extends keyof QqOfficialConfig>(
    key: K,
    value: QqOfficialConfig[K],
  ) {
    onChange({ ...config, official: { ...config.official, [key]: value } });
  }

  async function run(action: () => Promise<void>, okText: string) {
    setBusy(true);
    setNotice(null);
    try {
      await action();
      setNotice(okText);
      setStatus(await getQqStatus());
    } catch (e) {
      setNotice(`${e}`);
    } finally {
      setBusy(false);
    }
  }

  const { label, variant } = describe(status);
  const active = isActive(status);
  const blocked = busy || dirty;

  return (
    <>
      <Card
        eyebrow="QQ"
        title="连接状态"
        description="QQ 开放平台官方机器人，WebSocket 长连接，不需要公网回调地址。"
      >
        <div className={styles.statusRow}>
          <Badge variant={variant}>{label}</Badge>
          <div className={styles.statusActions}>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void run(testQqConnection, '凭证验证通过')}
              disabled={blocked}
            >
              测试连接
            </Button>
            {active ? (
              <>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void run(startQq, '已重新连接')}
                  disabled={blocked}
                >
                  重新连接
                </Button>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void run(stopQq, '已停止')}
                  disabled={busy}
                >
                  停止
                </Button>
              </>
            ) : (
              <Button
                variant="primary"
                size="sm"
                onClick={() => void run(startQq, '已启动')}
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
          群聊消息不支持流式，会等生成完成后按 2000 字符分片发送（同一消息最多 5
          条）；单聊短回复直接发普通消息，长回复才走原生流式，首帧失败会自动退回普通消息。
        </p>
        <p className={styles.hint}>
          流式消息目前只有手机 QQ 能渲染，电脑端 QQ 会显示「暂不支持该消息类型」。主要在电脑上使用时，
          建议把「单聊输出方式」改为非流式。
        </p>
        <p className={styles.hint}>
          个人主体机器人只能加入自己是群主或管理员的群，私聊需要先在开放平台添加「开发体验用户」。
        </p>
      </Card>

      <Card
        eyebrow="凭证"
        title="开放平台应用"
        description="QQ 开放平台 → 机器人 → 开发设置。AppID / AppSecret 用于换取 access_token。"
      >
        <div className={styles.grid}>
          <Dropdown
            label="启用状态 enabled"
            options={ENABLED_OPTIONS}
            value={String(config.enabled)}
            onChange={(v) => patch('enabled', v === 'true')}
          />
          <Dropdown
            label="后端 backend"
            options={BACKEND_OPTIONS}
            value={config.backend}
            onChange={(v) => patch('backend', v as QqBackend)}
          />
          <Input
            label="AppID app_id"
            value={config.official.app_id}
            placeholder="开放平台机器人 AppID"
            onChange={(e) => patchOfficial('app_id', e.currentTarget.value)}
          />
          <Input
            label="AppSecret app_secret"
            type="password"
            value={config.official.app_secret}
            placeholder="可用环境变量 AVALON_QQ_APP_SECRET 覆盖"
            onChange={(e) => patchOfficial('app_secret', e.currentTarget.value)}
          />
          <Dropdown
            label="环境 sandbox"
            options={SANDBOX_OPTIONS}
            value={String(config.official.sandbox)}
            onChange={(v) => patchOfficial('sandbox', v === 'true')}
          />
          <Input
            label="请求超时 timeout_secs"
            type="number"
            step="5"
            value={config.official.timeout_secs}
            onChange={(e) => {
              const n = e.currentTarget.valueAsNumber;
              if (Number.isFinite(n)) patchOfficial('timeout_secs', n);
            }}
          />
        </div>
        <p className={styles.hint}>
          沙箱环境与正式环境的机器人相互独立，凭证不通用。上线前记得切到正式环境并核对
          AppID。OneBot 后端属于非官方协议，存在封号风险，本版本不实现：选择它后启动会直接报错。
        </p>
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
            onChange={(v) => patch('session_mode', v as QqSessionMode)}
          />
          <Input
            label="用户白名单 allow_users（逗号分隔，留空 = 不限制）"
            value={allowText}
            placeholder="用户 openid，逗号分隔"
            onChange={(e) => setAllowText(e.currentTarget.value)}
            onBlur={() => patch('allow_users', parseList(allowText))}
          />
          <Input
            label="主人 owner_open_id"
            value={config.owner_open_id}
            placeholder="留空则由第一个私聊用户自动填充"
            onChange={(e) => patch('owner_open_id', e.currentTarget.value)}
          />
          <Input
            label="收到图片后的询问文本 image_ack_text（留空 = 立即处理）"
            value={config.image_ack_text}
            placeholder="已收到图片，请问需要我做什么？"
            onChange={(e) => patch('image_ack_text', e.currentTarget.value)}
          />
          <Dropdown
            label="单聊输出方式 stream_c2c"
            options={STREAM_OPTIONS}
            value={String(config.official.stream_c2c)}
            onChange={(v) => patchOfficial('stream_c2c', v === 'true')}
          />
        </div>
        <p className={styles.hint}>
          流式仅手机 QQ 可见；PC QQ 会把它显示成「暂不支持该消息类型」。短回复不足 24
          字时会自动改用普通消息，这类回复在电脑端也能正常看到。
        </p>
        <p className={styles.hint}>
          普通回复与流式回复都优先按 Markdown 渲染（加粗、列表、代码块等）；账号不支持时自动退回纯文本。
        </p>
        <p className={styles.hint}>
          用户身份是 openid，不是 QQ 号；openid 按应用隔离，换 AppID 后白名单必须重新配置。
        </p>
        <p className={styles.hint}>
          QQ 的图片和文字通常分两条消息送达。填写询问文本后，收到纯图片只会先回复这句话并把图片挂起，
          等下一条文字再一起交给模型；连发多张只询问一次，超过 10 分钟未跟进则丢弃。
        </p>
      </Card>

      <Card
        eyebrow="订阅"
        title="事件 intents"
        description="默认只订阅群聊与单聊消息事件（GROUP_AND_C2C_EVENT）。"
      >
        <div className={styles.grid}>
          <Input
            label="intents（十进制）"
            type="number"
            step="1"
            value={config.official.intents}
            onChange={(e) => {
              const n = e.currentTarget.valueAsNumber;
              if (Number.isFinite(n)) patchOfficial('intents', n);
            }}
          />
        </div>
        <p className={styles.hint}>
          群聊 + 单聊消息 = 33554432（1 &lt;&lt; 25）。填 0 或负数会回退到这个默认值。
          修改后需要在开放平台同步订阅对应事件，否则连接会因权限不足被关闭。
        </p>
      </Card>
    </>
  );
}
