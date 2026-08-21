'use client';

/**
 * Skeleton（Design System V2）—— 消费 elevation-inset / surface-hover 的 shimmer 骨架。
 * 禁止调用方传 hex；reduced-motion 时 shimmer 动画由 CSS 关闭。
 */

import { SPACING } from '@/lib/design-tokens';

export interface DsSkeletonProps {
  variant?: 'text' | 'card' | 'circle' | 'bar';
  lines?: number;
  width?: string | number;
  height?: string | number;
  borderRadius?: number;
  className?: string;
}

export function Skeleton({
  variant = 'text',
  lines = 3,
  width,
  height,
  borderRadius,
  className,
}: DsSkeletonProps) {
  const radius = borderRadius ?? (variant === 'circle' ? 999 : 6);

  const blockStyle: React.CSSProperties = {
    background: 'var(--elevation-inset)',
    borderRadius: radius,
    ...(variant === 'circle'
      ? { width: height ?? 40, height: height ?? 40 }
      : { width: width ?? '100%', height: height ?? 12 }),
  };

  if (variant === 'card') {
    return (
      <div
        className={className}
        style={{
          padding: SPACING.md,
          borderRadius: 14,
          border: '1px solid var(--border-subtle)',
          background: 'var(--elevation-raised)',
          display: 'flex',
          flexDirection: 'column',
          gap: SPACING.sm,
        }}
      >
        <div className="anim-shimmer" style={{ ...blockStyle, height: 48, width: '100%' }} />
        <Skeleton lines={2} />
      </div>
    );
  }

  if (variant === 'text' && lines > 1) {
    return (
      <div className={className} style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
        {Array.from({ length: lines }, (_, i) => (
          <div
            key={i}
            className="anim-shimmer"
            style={{
              ...blockStyle,
              width: i === lines - 1 ? '60%' : width ?? '100%',
              animationDelay: `${i * 80}ms`,
            }}
          />
        ))}
      </div>
    );
  }

  return <div className={`anim-shimmer ${className ?? ''}`} style={blockStyle} />;
}

export default Skeleton;
