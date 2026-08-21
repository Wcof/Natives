'use client';

/**
 * Empty（Design System V2）—— 空态基元，消费语义 token。
 * 文案由调用方提供（i18n 已在上层解析）。
 */

import type { ReactNode } from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';

export interface EmptyProps {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  action?: { label: ReactNode; onClick: () => void };
}

export function Empty({ icon, title, description, action }: EmptyProps) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: `${SPACING.xxl}px ${SPACING.xl}px`,
        textAlign: 'center',
        color: 'var(--text-secondary)',
      }}
    >
      {icon && (
        <div style={{ display: 'flex', justifyContent: 'center', marginBottom: SPACING.md, color: 'var(--text-tertiary)' }}>
          {icon}
        </div>
      )}
      <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.xs }}>
        {title}
      </div>
      {description && (
        <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', marginBottom: SPACING.lg, maxWidth: 280, lineHeight: 1.5 }}>
          {description}
        </div>
      )}
      {action && (
        <button className="btn btn-primary" onClick={action.onClick} style={{ fontSize: FONT_SIZE.sm }}>
          {action.label}
        </button>
      )}
    </div>
  );
}

export default Empty;
