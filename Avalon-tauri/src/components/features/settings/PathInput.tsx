import { open } from '@tauri-apps/plugin-dialog';
import { Button, Input } from '../../ui';
import styles from './PathInput.module.css';

/**
 * 弹出系统目录选择器，返回选中的绝对路径；用户取消则返回 null。
 *
 * 统一把反斜杠转成正斜杠：配置是 TOML，`\` 在里面是转义符，
 * Windows 选择器给回来的 `f:\Avalon\data` 直接写进去会让整份配置解析失败
 * （`\A`、`\d` 都是非法转义），下次启动就被兜底成默认配置。
 */
async function pickDirectory(defaultPath?: string): Promise<string | null> {
  const picked = await open({
    directory: true,
    multiple: false,
    // 带上当前值，再次点开时落在用户上次选的地方而不是回到根目录
    defaultPath: defaultPath?.trim() || undefined,
  });
  return typeof picked === 'string' ? picked.replace(/\\/g, '/') : null;
}

export interface PathInputProps {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (next: string) => void;
}

/**
 * 单个目录的输入。选择器是主要入口，但输入框保持可编辑 ——
 * 选择器给不了尚不存在的目录、UNC 路径，也没法粘贴
 */
export function PathInput({ label, value, placeholder, onChange }: PathInputProps) {
  async function browse() {
    const picked = await pickDirectory(value);
    if (picked) onChange(picked);
  }

  return (
    <div className={styles.row}>
      <Input
        className={styles.field}
        label={label}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.currentTarget.value)}
      />
      <Button variant="secondary" onClick={browse}>
        浏览…
      </Button>
    </div>
  );
}

export interface PathListInputProps {
  label: string;
  value: string[];
  onChange: (next: string[]) => void;
  /** 列表为空时的说明。空列表在业务上通常有特殊含义，交给调用方描述 */
  emptyHint?: string;
}

/**
 * 目录列表。只能通过选择器添加 —— 这里的每一项都是一条安全边界，
 * 手打错一个字母的后果是「配了却不生效」，而选出来的路径必然真实存在
 */
export function PathListInput({ label, value, onChange, emptyHint }: PathListInputProps) {
  async function add() {
    const picked = await pickDirectory(value[value.length - 1]);
    if (!picked) return;
    // 同一个目录加两次没有意义，且会让下面的删除按钮指向不明
    if (value.includes(picked)) return;
    onChange([...value, picked]);
  }

  return (
    <div>
      <span className={styles.label}>{label}</span>
      {value.length === 0 ? (
        <p className={styles.empty}>{emptyHint ?? '尚未添加任何目录'}</p>
      ) : (
        <div className={styles.list}>
          {value.map((p, i) => (
            <div key={p} className={styles.item}>
              <span className={styles.path} title={p}>
                {p}
              </span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onChange(value.filter((_, j) => j !== i))}
              >
                移除
              </Button>
            </div>
          ))}
        </div>
      )}
      <div className={styles.actions}>
        <Button variant="secondary" size="sm" onClick={add}>
          添加目录…
        </Button>
      </div>
    </div>
  );
}
