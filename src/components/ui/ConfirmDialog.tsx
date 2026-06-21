'use client';

import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';
import { useHydrated } from '@/hooks/useHydrated';

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
  const mounted = useHydrated();
  

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
  if (!mounted) return null;

  return createPortal(
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 9999,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        background: 'rgba(0,0,0,0.45)',
        backdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))',
        WebkitBackdropFilter: 'blur(var(--glass-overlay-blur, 24px)) saturate(var(--glass-overlay-saturation, 150%))',
        animation: `fadeIn ${TRANSITION.normal}`,
      }}
      onClick={onCancel}
    >
      <div
        ref={dialogRef}
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        className="anim-dropIn"
        style={{
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          WebkitBackdropFilter: 'blur(var(--vibe-toolbar-blur, 22px)) saturate(var(--vibe-toolbar-saturation, 145%))',
          border: '0.0625rem solid var(--vibe-toolbar-border)',
          borderRadius: 'var(--radius)',
          boxShadow: '0 0 0 1px color-mix(in srgb, var(--accent) 12%, transparent), var(--vibe-toolbar-shadow)',
          padding: SPACING.xl, maxWidth: 400, width: '90vw',
        }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <h3 style={{ fontSize: FONT_SIZE.xl, fontWeight: 600, color: 'var(--vibe-brand-text)', marginBottom: SPACING.sm }}>
          {title}
        </h3>
        <p style={{ fontSize: FONT_SIZE.lg, color: 'var(--vibe-brand-text)', marginBottom: SPACING.lg, lineHeight: 1.5, wordBreak: 'break-word', overflowWrap: 'break-word' }}>
          {message}
        </p>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: SPACING.sm }}>
          <button className="btn-ghost" onClick={onCancel}>{cancelLabel}</button>
          <button
            ref={confirmBtnRef}
            className="btn"
            onClick={onConfirm}
            style={{
              background: danger ? 'var(--danger)' : 'var(--accent)',
              color: danger ? 'var(--accent-ink)' : 'var(--accent-ink)',
              border: 'none', padding: `${SPACING.xs}px ${SPACING.md}px`, borderRadius: BORDER_RADIUS.md, cursor: 'pointer',
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
