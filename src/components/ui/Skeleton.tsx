'use client';

/**
 * Skeleton —— V2 委托版：转交给 design-system 的 Skeleton（消费语义 token）。
 * 保留旧 props 兼容（lines / variant / width / height），
 * variant: text|card|avatar|table → ds text|card|circle|bar。
 */

import { Skeleton as DsSkeleton } from '@/components/ui/design-system';

interface SkeletonProps {
  /** Number of text lines (default: 3) */
  lines?: number;
  /** Visual variant */
  variant?: 'text' | 'card' | 'avatar' | 'table';
  /** Width override */
  width?: string | number;
  /** Height override */
  height?: string | number;
}

export default function Skeleton({ lines = 3, variant = 'text', width, height }: SkeletonProps) {
  if (variant === 'card') {
    return <DsSkeleton variant="card" lines={lines} />;
  }

  if (variant === 'avatar') {
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <DsSkeleton variant="circle" height={height ?? 40} width={40} />
        <div style={{ flex: 1 }}>
          <DsSkeleton variant="text" lines={lines} />
        </div>
      </div>
    );
  }

  if (variant === 'table') {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {Array.from({ length: lines }, (_, i) => (
          <div
            key={i}
            style={{
              display: 'flex',
              gap: 12,
              alignItems: 'center',
              padding: '8px 0',
              borderBottom: '1px solid var(--border-subtle)',
            }}
          >
            <DsSkeleton variant="bar" width={24} height={12} />
            <DsSkeleton variant="bar" height={12} />
            <DsSkeleton variant="bar" width={60} height={12} />
          </div>
        ))}
      </div>
    );
  }

  return <DsSkeleton variant="text" lines={lines} width={width} height={height} />;
}
