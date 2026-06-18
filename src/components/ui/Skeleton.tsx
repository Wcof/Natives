'use client';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

/**
 * Skeleton loading placeholder — shows animated gray bars
 * instead of "Loading..." text for better perceived performance.
 *
 * Usage:
 *   <Skeleton lines={3} />
 *   <Skeleton variant="card" />
 *   <Skeleton variant="avatar" />
 */

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
    return (
      <div style={{
        padding: SPACING.md, borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-btn-border)',
        background: 'var(--vibe-toolbar-bg)',
      }}>
        <div style={{
          width: '100%', aspectRatio: '1', borderRadius: BORDER_RADIUS.sm,
          background: 'var(--vibe-btn-bg)',
          marginBottom: SPACING.sm,
          animation: 'skeleton-pulse 1.5s ease-in-out infinite',
        }} />
        <Skeleton lines={2} />
      </div>
    );
  }

  if (variant === 'avatar') {
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.md }}>
        <div style={{
          width: 40, height: 40, borderRadius: '50%',
          background: 'var(--vibe-btn-bg)',
          animation: 'skeleton-pulse 1.5s ease-in-out infinite',
          flexShrink: 0,
        }} />
        <div style={{ flex: 1 }}>
          <Skeleton lines={2} />
        </div>
      </div>
    );
  }

  if (variant === 'table') {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {Array.from({ length: lines }, (_, i) => (
          <div key={i} style={{
            display: 'flex', gap: SPACING.md, alignItems: 'center',
            padding: `${SPACING.sm}px 0`,
            borderBottom: '0.0625rem solid var(--vibe-btn-border)',
          }}>
            <div style={{
              width: 24, height: 12, borderRadius: 2,
              background: 'var(--vibe-btn-bg)',
              animation: 'skeleton-pulse 1.5s ease-in-out infinite',
              animationDelay: `${i * 0.1}s`,
            }} />
            <div style={{
              flex: 1, height: 12, borderRadius: 2,
              background: 'var(--vibe-btn-bg)',
              animation: 'skeleton-pulse 1.5s ease-in-out infinite',
              animationDelay: `${i * 0.1 + 0.05}s`,
            }} />
            <div style={{
              width: 60, height: 12, borderRadius: 2,
              background: 'var(--vibe-btn-bg)',
              animation: 'skeleton-pulse 1.5s ease-in-out infinite',
              animationDelay: `${i * 0.1 + 0.1}s`,
            }} />
          </div>
        ))}
      </div>
    );
  }

  // Default: text lines
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm, width }}>
      {Array.from({ length: lines }, (_, i) => (
        <div
          key={i}
          style={{
            height: height || 12,
            width: i === lines - 1 ? '60%' : '100%',
            borderRadius: BORDER_RADIUS.sm,
            background: 'var(--vibe-btn-bg)',
            animation: 'skeleton-pulse 1.5s ease-in-out infinite',
            animationDelay: `${i * 0.1}s`,
          }}
        />
      ))}
    </div>
  );
}
