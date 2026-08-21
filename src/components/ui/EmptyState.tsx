'use client';

/**
 * EmptyState / LoadingState / ErrorState / Skeleton —— V2 委托版。
 * 统一转交 design-system primitives（消费语义 token，禁止 hex）。
 * 保留旧导出与 props 兼容。
 */

import { type ReactNode } from 'react';
import { Inbox } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { Empty as DsEmpty, ErrorPrimitive as DsError, Skeleton as DsSkeleton } from '@/components/ui/design-system';
import { MathCurveLoader } from './MathCurveLoader';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';

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
    <DsEmpty icon={renderedIcon} title={title} description={description} action={action} />
  );
}

// ── Loading State ──

interface LoadingStateProps {
  message?: string;
}

export function LoadingState({ message }: LoadingStateProps) {
  const locale = useLocale();
  const text = message ?? t(locale, 'common.loading');
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: SPACING.xxl,
        gap: SPACING.md,
      }}
    >
      <MathCurveLoader size={60} />
      <span style={{ fontSize: FONT_SIZE.md, color: 'var(--text-tertiary)', letterSpacing: '0.03em' }}>{text}</span>
    </div>
  );
}

// ── Error State ──

interface ErrorStateProps {
  message: string;
  onRetry?: () => void;
  icon?: ReactNode;
  /** 覆盖重试按钮文案；默认 common.retry（随语言切换） */
  retryLabel?: string;
}

export function ErrorState({ message, onRetry, icon, retryLabel }: ErrorStateProps) {
  const locale = useLocale();
  return (
    <DsError
      message={message}
      onRetry={onRetry}
      icon={icon}
      retryLabel={retryLabel ?? t(locale, 'common.retry')}
    />
  );
}

// ── Skeleton ──

interface SkeletonProps {
  width?: string | number;
  height?: string | number;
  count?: number;
  borderRadius?: number;
}

export function Skeleton({ width = '100%', height = 12, count = 1, borderRadius }: SkeletonProps) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
      {Array.from({ length: count }).map((_, i) => (
        <DsSkeleton key={i} variant="bar" width={width} height={height} borderRadius={borderRadius} />
      ))}
    </div>
  );
}
