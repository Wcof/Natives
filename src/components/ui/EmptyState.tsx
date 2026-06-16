'use client';

import { type ReactNode } from 'react';
import { Inbox, AlertTriangle, RefreshCw } from 'lucide-react';

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
      padding: '40px 20px', textAlign: 'center',
    }}>
      <div style={{ display: 'flex', justifyContent: 'center', marginBottom: 12 }}>{renderedIcon}</div>
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
        <span className="anim-livePulse" style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--accent,#FFF5E6)' }} />
        <span style={{ fontSize: 12, color: 'var(--text-faint)' }}>{message}</span>
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
      padding: 40, textAlign: 'center',
    }}>
      <div style={{ display: 'flex', justifyContent: 'center', marginBottom: 8 }}>{renderedIcon}</div>
      <div style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 12 }}>{message}</div>
      {onRetry && (
        <button className="btn" onClick={onRetry} style={{ fontSize: 11 }}>
          <RefreshCw size={14} style={{ marginRight: 4 }} /> Retry
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
