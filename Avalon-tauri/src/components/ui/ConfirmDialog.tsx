// 确认对话框（confirm dialog）：基于 Modal 的二选一确认，用于替代 window.confirm。
// 危险操作（danger）时确认按钮变红；默认聚焦取消按钮（data-autofocus），避免回车误触破坏性操作。

import type { ReactNode } from 'react';
import { Button } from './Button';
import { Modal } from './Modal';

export interface ConfirmDialogProps {
  open: boolean;
  title: ReactNode;
  description?: ReactNode;
  /** 确认按钮文字，默认「确认」 */
  confirmText?: string;
  /** 取消按钮文字，默认「取消」 */
  cancelText?: string;
  /** 危险操作：确认按钮用红色 */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmText = '确认',
  cancelText = '取消',
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={title}
      description={description}
      footer={
        <>
          <Button variant="secondary" data-autofocus onClick={onCancel}>
            {cancelText}
          </Button>
          <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm}>
            {confirmText}
          </Button>
        </>
      }
    />
  );
}
