'use client';

/**
 * MaterialSurface —— Dark Glow 材质表面。
 * 低对比深色：surface→surface-active 的克制纵向渐变，无霓虹墙。
 * glow 仅在 tone !== 'none' 且 active 时出现（focus/selected/status）。
 */

import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';
import { Surface, type SurfaceRadius } from './Surface';
import type { ElevationLevel } from '@/lib/design-tokens';

export type GlowTone = 'none' | 'focus' | 'selected' | 'success' | 'danger';

const GLOW_VAR: Record<Exclude<GlowTone, 'none'>, string> = {
  focus: 'var(--glow-focus)',
  selected: 'var(--glow-selected)',
  success: 'var(--glow-status-success)',
  danger: 'var(--glow-status-danger)',
};

export interface MaterialSurfaceProps extends Omit<HTMLAttributes<HTMLDivElement>, 'children'> {
  elevation?: ElevationLevel;
  radius?: SurfaceRadius;
  /** 克制 glow 的语义 tone；active=false 时不发光。 */
  tone?: GlowTone;
  active?: boolean;
  children?: ReactNode;
  style?: CSSProperties;
}

export function MaterialSurface({
  elevation = 'raised',
  radius = 'lg',
  tone = 'none',
  active = false,
  style,
  children,
  ...rest
}: MaterialSurfaceProps) {
  const glow = tone !== 'none' && active ? GLOW_VAR[tone] : undefined;
  const baseShadow =
    elevation === 'inset' ? 'var(--elev-shadow-inset)' : 'var(--elev-shadow-raised)';

  const shadow = glow ? [glow, baseShadow].join(', ') : baseShadow;

  return (
    <Surface
      {...rest}
      elevation={elevation}
      radius={radius}
      style={{
        background: 'linear-gradient(180deg, var(--elevation-raised) 0%, var(--elevation-base) 130%)',
        boxShadow: shadow,
        ...style,
      }}
    >
      {children}
    </Surface>
  );
}

export default MaterialSurface;
