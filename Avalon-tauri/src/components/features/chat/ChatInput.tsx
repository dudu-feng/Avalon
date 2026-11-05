import { useEffect, useRef, useState } from 'react';
import type { ChangeEvent, ClipboardEvent, DragEvent, KeyboardEvent } from 'react';
import { Button, CircleProgress, ConfirmDialog, Dropdown, Tooltip } from '../../ui';
import { saveAttachment } from '../../../lib/fileApi';
import type { ContextUsage, MessageAttachment } from '../../../types/chat';
import styles from './ChatInput.module.css';

export interface ModelOption {
  name: string;
  modelname: string;
}

export interface ChatInputProps {
  onSubmit: (text: string, attachments: MessageAttachment[]) => void;
  onStop: () => void;
  isBusy?: boolean;
  models?: ModelOption[];
  activeModel?: string;
  onModelChange?: (name: string) => void;
  contextUsage?: ContextUsage | null;
  /** 主动压缩当前上下文（进度圈点击确认后触发） */
  onCompress?: () => Promise<void> | void;
  /** 是否正在主动压缩（进度圈禁用 + 防止重复触发） */
  compressing?: boolean;
  /** 视觉能力可用时才显示图片附件入口 */
  visionEnabled?: boolean;
  /** 当前模型支持图片，但视觉总开关没开（提示去设置页启用） */
  visionCapableButDisabled?: boolean;
  /** 实际视觉模型标识（用于首次外发确认文案与按模型记录确认） */
  visionModel?: string;
  /** 首次向该视觉模型发送图片前是否弹确认 */
  confirmBeforeSend?: boolean;
}

/** textarea 自动增长的封顶高度（与 CSS 的 max-height 一致） */
const MAX_HEIGHT = 160;

/** 单条消息最多携带的图片数，与后端 MAX_ATTACHMENTS_PER_MESSAGE 一致 */
const MAX_ATTACHMENTS = 4;

/** 已确认过外发的视觉模型列表：同一模型只提示一次，换模型重新提示 */
const VISION_ACK_KEY = 'avalon.vision.acknowledged_models';

function acknowledgedModels(): string[] {
  try {
    const raw = localStorage.getItem(VISION_ACK_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((item): item is string => typeof item === 'string')
      : [];
  } catch {
    // localStorage 不可用或内容损坏时按未确认处理：宁可多问一次
    return [];
  }
}

function markModelAcknowledged(model: string) {
  try {
    const list = acknowledgedModels();
    if (!list.includes(model)) {
      localStorage.setItem(VISION_ACK_KEY, JSON.stringify([...list, model]));
    }
  } catch {
    // 持久化失败只影响下次是否重复确认，不阻断发送
  }
}

interface PendingAttachment {
  path: string;
  name: string;
  mime: string;
  size: number;
  sha256: string;
  /** 本地预览 URL（object URL），发送或移除时释放 */
  previewUrl: string;
}

export function ChatInput({
  onSubmit,
  onStop,
  isBusy = false,
  models = [],
  activeModel = '',
  onModelChange = () => {},
  contextUsage,
  onCompress,
  compressing = false,
  visionEnabled = false,
  visionCapableButDisabled = false,
  visionModel = '',
  confirmBeforeSend = true,
}: ChatInputProps) {
  const [value, setValue] = useState('');
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [stopConfirmOpen, setStopConfirmOpen] = useState(false);
  const [visionConfirmOpen, setVisionConfirmOpen] = useState(false);
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [uploading, setUploading] = useState(false);
  const [dragging, setDragging] = useState(false);
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const attachmentsRef = useRef<PendingAttachment[]>([]);
  /** 等待外发确认期间的发送内容；取消时保留输入框与附件 */
  const pendingSendRef = useRef<{ text: string; attachments: MessageAttachment[] } | null>(null);

  // 预览 URL 与组件生命周期绑定：移除/发送时主动释放，卸载时兜底
  useEffect(() => {
    attachmentsRef.current = attachments;
  }, [attachments]);
  useEffect(
    () => () => {
      attachmentsRef.current.forEach((a) => URL.revokeObjectURL(a.previewUrl));
    },
    [],
  );

  const revokeAll = (items: PendingAttachment[]) => {
    items.forEach((a) => URL.revokeObjectURL(a.previewUrl));
  };

  /** 选中/拖入/粘贴的图片：立即落盘，只把相对路径留到发送时 */
  const addFiles = async (list: FileList | File[]) => {
    // 部分系统/剪贴板来源的 File.type 为空，扩展名兜底；最终仍由后端内容嗅探把关
    const files = Array.from(list).filter(
      (f) => f.type.startsWith('image/') || /\.(png|jpe?g|webp|gif|bmp|tiff?)$/i.test(f.name),
    );
    if (files.length === 0) return;
    const room = MAX_ATTACHMENTS - attachmentsRef.current.length;
    if (room <= 0) return;

    setUploading(true);
    try {
      for (const file of files.slice(0, room)) {
        const previewUrl = URL.createObjectURL(file);
        try {
          const stored = await saveAttachment(file, { source: 'upload' });
          const pending: PendingAttachment = {
            path: stored.path,
            name: stored.original_name || file.name || '图片',
            mime: stored.mime,
            size: stored.size,
            sha256: stored.sha256,
            previewUrl,
          };
          setAttachments((prev) => {
            const next = [...prev, pending];
            attachmentsRef.current = next;
            return next;
          });
        } catch (e) {
          URL.revokeObjectURL(previewUrl);
          console.error('保存图片附件失败:', e);
        }
      }
    } finally {
      setUploading(false);
    }
  };

  const removeAttachment = (path: string) => {
    setAttachments((prev) => {
      const target = prev.find((a) => a.path === path);
      if (target) URL.revokeObjectURL(target.previewUrl);
      const next = prev.filter((a) => a.path !== path);
      attachmentsRef.current = next;
      return next;
    });
  };

  const clearAttachments = () => {
    revokeAll(attachmentsRef.current);
    attachmentsRef.current = [];
    setAttachments([]);
  };

  // 自动增长：先复位再按内容撑高，封顶后内部滚动
  const adjustHeight = () => {
    const field = fieldRef.current;
    if (!field) return;
    field.style.height = 'auto';
    field.style.height = `${Math.min(field.scrollHeight, MAX_HEIGHT)}px`;
  };

  const sendNow = (text: string, payload: MessageAttachment[]) => {
    onSubmit(text, payload);
    setValue('');
    clearAttachments();
    // 清空后复位为单行（height:auto 随空内容缩回）
    if (fieldRef.current) fieldRef.current.style.height = 'auto';
  };

  const submit = () => {
    if (visionConfirmOpen) return;
    const trimmed = value.trim();
    if ((!trimmed && attachments.length === 0) || isBusy || uploading) return;

    const payload: MessageAttachment[] = attachments.map((a) => ({
      path: a.path,
      mime: a.mime,
      size: a.size,
      sha256: a.sha256,
      original_name: a.name,
    }));

    // 图片会离开本机：首次向该视觉模型发送前确认一次
    if (
      payload.length > 0 &&
      confirmBeforeSend &&
      visionModel &&
      !acknowledgedModels().includes(visionModel)
    ) {
      pendingSendRef.current = { text: trimmed, attachments: payload };
      setVisionConfirmOpen(true);
      return;
    }

    sendNow(trimmed, payload);
  };

  const confirmVisionSend = () => {
    if (visionModel) markModelAcknowledged(visionModel);
    setVisionConfirmOpen(false);
    const pending = pendingSendRef.current;
    pendingSendRef.current = null;
    if (pending) sendNow(pending.text, pending.attachments);
  };

  const cancelVisionSend = () => {
    setVisionConfirmOpen(false);
    pendingSendRef.current = null;
  };

  // 停止前确认：后端会丢弃本轮，必须让用户明确知道本轮历史不会保存
  const handleStop = () => {
    setStopConfirmOpen(false);
    onStop();
  };

  // 确认框确认：关弹窗后触发主动压缩（错误仅记日志，不打断输入流程）
  const handleCompress = async () => {
    setConfirmOpen(false);
    if (!onCompress || compressing) return;
    try {
      await onCompress();
    } catch (e) {
      console.error('压缩上下文失败:', e);
    }
  };

  const onChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    setValue(e.currentTarget.value);
    adjustHeight();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter 发送（Shift+Enter 换行）；IME 组合态中的 Enter 不触发
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  // 粘贴截图：剪贴板里的图片没有本地路径，直接交给 saveAttachment 落盘
  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    if (!visionEnabled) return;
    const files = e.clipboardData.files;
    if (files.length > 0) {
      e.preventDefault();
      void addFiles(files);
    }
  };

  const onDragOver = (e: DragEvent<HTMLDivElement>) => {
    if (!visionEnabled) return;
    if (e.dataTransfer.types.includes('Files')) {
      e.preventDefault();
      setDragging(true);
    }
  };

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    if (!visionEnabled) return;
    e.preventDefault();
    setDragging(false);
    void addFiles(e.dataTransfer.files);
  };

  return (
    <>
      <div
        className={`${styles.card} ${dragging ? styles.dragging : ''}`}
        onDragOver={onDragOver}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
      >
        {attachments.length > 0 && (
          <div className={styles.attachments}>
            {attachments.map((a) => (
              <div key={a.path} className={styles.attachment}>
                <img className={styles.attachmentThumb} src={a.previewUrl} alt={a.name} />
                <span className={styles.attachmentName} title={a.name}>
                  {a.name}
                </span>
                <button
                  type="button"
                  className={styles.attachmentRemove}
                  onClick={() => removeAttachment(a.path)}
                  aria-label={`移除 ${a.name}`}
                  title="移除"
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        )}
        <textarea
          ref={fieldRef}
          className={styles.field}
          value={value}
          rows={1}
          placeholder={
            visionEnabled
              ? '输入消息，Enter 发送，Shift+Enter 换行；可粘贴/拖入图片…'
              : '输入消息，Enter 发送，Shift+Enter 换行…'
          }
          onChange={onChange}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
        />
        <div className={styles.toolbar}>
          <div className={styles.left}>
            {visionEnabled && (
              <>
                <input
                  ref={fileRef}
                  type="file"
                  accept="image/*"
                  multiple
                  hidden
                  onChange={(e) => {
                    if (e.currentTarget.files) void addFiles(e.currentTarget.files);
                    e.currentTarget.value = '';
                  }}
                />
                <Tooltip label={`添加图片（最多 ${MAX_ATTACHMENTS} 张）`}>
                  <button
                    type="button"
                    className={styles.attachButton}
                    onClick={() => fileRef.current?.click()}
                    disabled={uploading || attachments.length >= MAX_ATTACHMENTS}
                    aria-label="添加图片"
                  >
                    {uploading ? '上传中…' : '＋图片'}
                  </button>
                </Tooltip>
              </>
            )}
            {contextUsage && (
              <Tooltip
                label={`上下文 ${contextUsage.used_tokens} / ${contextUsage.threshold} tokens · 点击压缩`}
              >
                <button
                  type="button"
                  className={styles.usageButton}
                  onClick={() => setConfirmOpen(true)}
                  disabled={compressing}
                  title="压缩当前上下文"
                  aria-label="压缩当前上下文"
                >
                  <CircleProgress
                    value={contextUsage.used_tokens}
                    max={contextUsage.threshold}
                    size={18}
                    strokeWidth={3}
                    label=""
                  />
                </button>
              </Tooltip>
            )}
            <span className={styles.hint}>
              {visionCapableButDisabled
                ? '当前模型支持图片，但图片输入未启用（设置 → 视觉）'
                : 'Enter 发送 · Shift+Enter 换行'}
            </span>
          </div>
          <div className={styles.right}>
            {models.length > 0 && (
              <Dropdown
                options={models.map((m) => ({ value: m.name, label: m.name }))}
                value={activeModel}
                onChange={onModelChange}
                align="end"
              />
            )}
            <Button
              size="sm"
              variant={isBusy ? 'secondary' : 'primary'}
              onClick={isBusy ? () => setStopConfirmOpen(true) : submit}
              disabled={!isBusy && !value.trim() && attachments.length === 0}
            >
              {isBusy ? '停止' : '发送'}
            </Button>
          </div>
        </div>
      </div>
      <ConfirmDialog
        open={confirmOpen}
        title="压缩上下文？"
        description="将当前会话的未压缩消息压缩为摘要，释放上下文空间。压缩后的原始内容仍会保留在历史记录中。"
        confirmText="压缩"
        onConfirm={handleCompress}
        onCancel={() => setConfirmOpen(false)}
      />
      <ConfirmDialog
        open={stopConfirmOpen}
        title="停止生成？"
        description="中断会丢失本轮对话历史：你刚发送的消息、已生成的内容和工具调用都不会被保存。"
        confirmText="停止"
        danger
        onConfirm={handleStop}
        onCancel={() => setStopConfirmOpen(false)}
      />
      <ConfirmDialog
        open={visionConfirmOpen}
        title="发送图片到模型服务商？"
        description={`图片会先在本机缩放并去除 EXIF 元数据，然后发送给视觉模型${
          visionModel ? `「${visionModel}」` : ''
        }。请确认该服务商可信，且图片不含敏感信息。此确认每个模型只出现一次。`}
        confirmText="发送"
        onConfirm={confirmVisionSend}
        onCancel={cancelVisionSend}
      />
    </>
  );
}
