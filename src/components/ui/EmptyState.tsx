'use client';

import { type ReactNode } from 'react';
import { Inbox, AlertTriangle, RefreshCw } from 'lucide-react';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

// ── Empty State (TASK-017) ──

interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: { label: string; onClick: () => void };
}

export function EmptyState({ icon, title, description, action }: EmptyStateProps) {
  const renderedIcon = icon !== undefined ? icon : <Inbox size={32} />;
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      padding: `${SPACING.xxl}px ${SPACING.xl}px`, textAlign: 'center',
    }}>
      <div style={{ display: 'flex', justifyContent: 'center', marginBottom: SPACING.md }}>{renderedIcon}</div>
      <div style={{ fontSize: FONT_SIZE.xl, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.xs }}>{title}</div>
      {description && <div style={{ fontSize: FONT_SIZE.md, color: 'var(--text-faint)', marginBottom: SPACING.lg, maxWidth: 280 }}>{description}</div>}
      {action && (
        <button className="btn btn-primary" onClick={action.onClick} style={{ fontSize: FONT_SIZE.md }}>
          {action.label}
        </button>
      )}
    </div>
  );
}

// ── Loading State ──

interface LoadingStateProps {
  message?: string;
}

export function LoadingState({ message = 'Loading...' }: LoadingStateProps) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      padding: SPACING.xxl,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm }}>
        <span className="anim-livePulse" style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--accent)' }} />
        <span style={{ fontSize: FONT_SIZE.md, color: 'var(--text-faint)' }}>{message}</span>
      </div>
    </div>
  );
}

// ── Error State ──

interface ErrorStateProps {
  message: string;
  onRetry?: () => void;
  icon?: ReactNode;
}

export function ErrorState({ message, onRetry, icon }: ErrorStateProps) {
  const renderedIcon = icon !== undefined ? icon : <AlertTriangle size={28} style={{ color: 'var(--warning)' }} />;
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      padding: SPACING.xxl, textAlign: 'center',
    }}>
      <div style={{ display: 'flex', justifyContent: 'center', marginBottom: SPACING.sm }}>{renderedIcon}</div>
      <div style={{ fontSize: FONT_SIZE.md, color: 'var(--text-dim)', marginBottom: SPACING.md }}>{message}</div>
      {onRetry && (
        <button className="btn" onClick={onRetry} style={{ fontSize: FONT_SIZE.sm }}>
          <RefreshCw size={14} style={{ marginRight: SPACING.xs }} /> Retry
        </button>
      )}
    </div>
  );
}

// ── Skeleton ──

interface SkeletonProps {
  width?: string | number;
  height?: string | number;
  count?: number;
  borderRadius?: number;
}

export function Skeleton({ width = '100%', height = 12, count = 1, borderRadius = 4 }: SkeletonProps) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="anim-livePulse" style={{
          width: typeof width === 'number' ? `${width}px` : width,
          height: typeof height === 'number' ? `${height}px` : height,
          borderRadius,
          background: 'var(--vibe-btn-bg)',
        }} />
      ))}
    </div>
  );
}
