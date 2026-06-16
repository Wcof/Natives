'use client';

import { useEffect, useRef } from 'react';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  danger?: boolean; // 红色确认按钮（删除/卸载场景）
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * 通用确认对话框 —— 替代 window.confirm()
 *
 * 键盘可用性：
 * - Tab / Shift+Tab 在按钮间循环
 * - Enter 触发确认
 * - Escape 触发取消
 */
export default function ConfirmDialog({
  open, title, message, confirmLabel, cancelLabel, danger, onConfirm, onCancel,
}: ConfirmDialogProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const confirmBtnRef = useRef<HTMLButtonElement>(null);

  // Focus trap: 打开时聚焦确认按钮，关闭后恢复
  useEffect(() => {
    if (open) {
      // 延迟一帧确保 DOM 已渲染
      requestAnimationFrame(() => {
        confirmBtnRef.current?.focus();
      });
    }
  }, [open]);

  // Escape 键关闭
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onCancel();
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [open, onCancel]);

  // Tab 循环焦点
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'Tab' || !dialogRef.current) return;
    const buttons = dialogRef.current.querySelectorAll<HTMLButtonElement>('button');
    if (buttons.length < 2) return;
    const first = buttons[0]!;
    const last = buttons[buttons.length - 1]!;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  if (!open) return null;

  return (
    <div
      style={{
        position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)',
        display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60,
      }}
      onClick={onCancel}
    >
      <div
        ref={dialogRef}
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{
          background: 'var(--bg-2)', border: '1px solid var(--border)',
          borderRadius: 8, padding: 20, maxWidth: 400, width: '90vw',
        }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <h3 style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 8 }}>
          {title}
        </h3>
        <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 16, lineHeight: 1.5 }}>
          {message}
        </p>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button className="btn-ghost" onClick={onCancel}>{cancelLabel}</button>
          <button
            ref={confirmBtnRef}
            className="btn"
            onClick={onConfirm}
            style={{
              background: danger ? 'var(--danger)' : 'var(--accent)',
              color: danger ? '#fff' : 'var(--accent-ink)',
              border: 'none', padding: '6px 14px', borderRadius: 6, cursor: 'pointer',
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
