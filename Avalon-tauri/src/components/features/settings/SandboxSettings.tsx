import { useEffect, useState } from 'react';
import { Card, Dropdown, Input } from '../../ui';
import { PathListInput } from './PathInput';
import type { ToolsConfig } from '../../../types/config';
import styles from './SandboxSettings.module.css';

export interface SandboxSettingsProps {
  config: ToolsConfig;
  onChange: (next: ToolsConfig) => void;
}

/** workspace_roots 的三态。后端靠「字段缺失 / 空数组 / 非空数组」区分，UI 得把它显式化 */
type RootMode = 'default' | 'custom' | 'off';

const ROOT_MODES = [
  { value: 'default', label: '默认工作区（data/workspace）' },
  { value: 'custom', label: '自定义目录' },
  { value: 'off', label: '禁止全部文件操作' },
];

function modeOf(roots: ToolsConfig['workspace_roots']): RootMode {
  if (roots == null) return 'default';
  return roots.length === 0 ? 'off' : 'custom';
}

/** 命令名不含空格，按逗号或空白切都行 */
function parseCommands(text: string): string[] {
  return text
    .split(/[,，\s]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** Agent 基础工具的沙箱：文件工作区边界 + 终端命令白名单 */
export function SandboxSettings({ config, onChange }: SandboxSettingsProps) {
  // 模式必须独立存一份，不能纯从配置推。空数组是二义的：
  // 既是「禁止全部」，也是「刚切到自定义、还没添加目录」。
  // 纯推导的话，切到自定义会立刻被判回禁止，选择器根本没机会出现
  const [mode, setMode] = useState<RootMode>(() => modeOf(config.workspace_roots));

  // 命令白名单在本地以文本编辑，失焦时才解析回数组 ——
  // 否则每敲一个逗号就会被 split 掉，没法接着输入下一项。
  // 目录列表不需要这套：它只能从选择器进来，没有逐字输入的过程
  const [cmdText, setCmdText] = useState(() => config.shell_allowlist.join(', '));

  // 外部重新加载配置时同步回文本框（也顺带把失焦解析的结果规范化显示）
  useEffect(() => {
    setCmdText(config.shell_allowlist.join(', '));
  }, [config.shell_allowlist]);

  // 外部重载时同步模式，但撞上那个二义的空数组时保留用户当前的选择
  useEffect(() => {
    const derived = modeOf(config.workspace_roots);
    setMode((cur) => (derived === 'off' && cur === 'custom' ? 'custom' : derived));
  }, [config.workspace_roots]);

  function switchMode(next: RootMode) {
    setMode(next);
    if (next === 'default') return onChange({ ...config, workspace_roots: null });
    if (next === 'off') return onChange({ ...config, workspace_roots: [] });
    // 切到自定义时先给个空数组占位（此刻等同于全禁），用户再逐个添加目录。
    // PathListInput 的 emptyHint 会提示这个中间状态，免得以为切过来就生效了
    onChange({ ...config, workspace_roots: config.workspace_roots ?? [] });
  }

  return (
    <>
      <Card
        eyebrow="沙箱"
        title="文件工作区"
        description="read_file / write_file / delete_file / get_directory_contents 只能访问这里列出的目录及其子目录，读写同一条边界。"
      >
        <div className={styles.grid}>
          <Dropdown
            label="访问范围 workspace_roots"
            options={ROOT_MODES}
            value={mode}
            onChange={(v) => switchMode(v as RootMode)}
          />
        </div>

        {mode === 'custom' && (
          <div className={styles.roots}>
            <PathListInput
              label="允许访问的目录"
              value={config.workspace_roots ?? []}
              emptyHint="还没添加任何目录，当前效果等同于「禁止全部文件操作」。"
              onChange={(next) => onChange({ ...config, workspace_roots: next })}
            />
          </div>
        )}

        {mode === 'off' && (
          <p className={styles.warn}>
            当前禁止全部文件操作。模型仍会看到这四个工具，但每次调用都会被拒绝。
          </p>
        )}

        <p className={styles.hint}>
          目录从选择器添加，选出来的路径必然真实存在 —— 手打的路径一旦有错字，
          表现是「配了却不生效」，很难联想到是拼写问题。列表改动立即生效，不用重启。
        </p>
        <p className={styles.hint}>
          放进来的目录，模型就能读也能删。不建议加入包含密钥、凭证的目录 ——
          它读到的内容可以经飞书工具发出去，而一段被抓取的网页正文就足以诱导它这么做。
        </p>
      </Card>

      <Card
        eyebrow="沙箱"
        title="终端命令白名单"
        description="run_shell_command 只能执行这里列出的命令。命令不经过 shell，参数逐个传递。"
      >
        <div className={styles.grid}>
          <Input
            className={styles.wide}
            label="允许的命令 shell_allowlist（逗号分隔，留空 = 禁用终端）"
            value={cmdText}
            placeholder="where, ping, ipconfig, tasklist"
            onChange={(e) => setCmdText(e.currentTarget.value)}
            onBlur={() => onChange({ ...config, shell_allowlist: parseCommands(cmdText) })}
          />
        </div>

        {config.shell_allowlist.length === 0 && (
          <p className={styles.warn}>当前白名单为空，终端功能已禁用。</p>
        )}

        <p className={styles.hint}>
          只填命令名，不带路径和参数（写 ping，不是 ping -n 1）。因为不经过 shell，
          &amp;&amp; | &gt; ; 这些符号没有特殊含义，管道和重定向都用不了；
          只解析 .exe/.com，npm、conda 这类脚本包装无法调用。dir、type 是 cmd
          内建命令，没有对应的可执行文件，填了也用不了 —— 它们的功能 read_file 与
          get_directory_contents 已经覆盖。
        </p>
        <p className={styles.hint}>
          往里加命令前请想清楚这层性质：一旦放进解释器（python、node、powershell）
          或带插件机制的工具（git 的 -c core.pager、npm 的 run 脚本），限制就从「防恶意」
          退化成「防误操作」—— 一行代码即可绕开上面的工作区边界。同理不建议加 findstr，
          它的 /f: 参数能读任意文件。
        </p>
      </Card>
    </>
  );
}
