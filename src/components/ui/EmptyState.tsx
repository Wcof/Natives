'use client';

import { type ReactNode } from 'react';

// ── Empty State (TASK-017) ──

interface EmptyStateProps {
  icon?: string;
  title: string;
  description?: string;
  action?: { label: string; onClick: () => void };
}

export function EmptyState({ icon = '📭', title, description, action }: EmptyStateProps) {
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      padding: '40px 20px', textAlign: 'center',
    }}>
      <div style={{ fontSize: 32, marginBottom: 12 }}>{icon}</div>
      <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>{title}</div>
      {description && <div style={{ fontSize: 12, color: 'var(--text-faint)', marginBottom: 16, maxWidth: 280 }}>{description}</div>}
      {action && (
        <button className="btn btn-primary" onClick={action.onClick} style={{ fontSize: 12 }}>
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
      padding: 40,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span className="anim-livePulse" style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--accent,#cdf24b)' }} />
        <span style={{ fontSize: 12, color: 'var(--text-faint)' }}>{message}</span>
      </div>
    </div>
  );
}

// ── Error State ──

interface ErrorStateProps {
  message: string;
  onRetry?: () => void;
  icon?: string;
}

export function ErrorState({ message, onRetry, icon = '⚠️' }: ErrorStateProps) {
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      padding: 40, textAlign: 'center',
    }}>
      <div style={{ fontSize: 28, marginBottom: 8 }}>{icon}</div>
      <div style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 12 }}>{message}</div>
      {onRetry && (
        <button className="btn" onClick={onRetry} style={{ fontSize: 11 }}>
          ↻ Retry
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
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="anim-livePulse" style={{
          width: typeof width === 'number' ? `${width}px` : width,
          height: typeof height === 'number' ? `${height}px` : height,
          borderRadius,
          background: 'var(--bg-3,#1c1e17)',
        }} />
      ))}
    </div>
  );
}
