'use client';

/**
 * GlowEdge —— 克制 glow 容器。只用于 focus/selected/status，
 * 禁止把 glow 当普通装饰铺满界面（无霓虹墙）。
 */

import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';

export type GlowTone = 'focus' | 'selected' | 'success' | 'danger';

const GLOW_VAR: Record<GlowTone, string> = {
  focus: 'var(--glow-focus)',
  selected: 'var(--glow-selected)',
  success: 'var(--glow-status-success)',
  danger: 'var(--glow-status-danger)',
};

export interface GlowEdgeProps extends HTMLAttributes<HTMLDivElement> {
  tone?: GlowTone;
  /** 是否激活发光（selected/focus 状态切换）。 */
  active?: boolean;
  radius?: number;
  children?: ReactNode;
  style?: CSSProperties;
}

export function GlowEdge({
  tone = 'focus',
  active = true,
  radius = 12,
  children,
  style,
  ...rest
}: GlowEdgeProps) {
  return (
    <div
      {...rest}
      style={{
        borderRadius: radius,
        boxShadow: active ? GLOW_VAR[tone] : undefined,
        transition: 'box-shadow 200ms cubic-bezier(0.16, 1, 0.3, 1)',
        ...style,
      }}
    >
      {children}
    </div>
  );
}

export default GlowEdge;
