'use client';

/**
 * Error（Design System V2）—— 错误态基元，消费语义 token。
 * 文案由调用方提供（i18n 已在上层解析）。
 */

import type { ReactNode } from 'react';
import { RefreshCw, AlertTriangle } from 'lucide-react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';

export interface ErrorProps {
  message: ReactNode;
  onRetry?: () => void;
  retryLabel?: ReactNode;
  icon?: ReactNode;
}

export function Error({ message, onRetry, retryLabel, icon }: ErrorProps) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: SPACING.xxl,
        textAlign: 'center',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'center', marginBottom: SPACING.sm }}>
        {icon ?? <AlertTriangle size={28} style={{ color: 'var(--warning)' }} />}
      </div>
      <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', marginBottom: SPACING.md, maxWidth: 320, lineHeight: 1.5 }}>
        {message}
      </div>
      {onRetry && (
        <button className="btn" onClick={onRetry} style={{ fontSize: FONT_SIZE.sm }}>
          <RefreshCw size={14} style={{ marginRight: SPACING.xs }} /> {retryLabel ?? 'Retry'}
        </button>
      )}
    </div>
  );
}

export default Error;
