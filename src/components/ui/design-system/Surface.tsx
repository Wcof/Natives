'use client';

/**
 * Surface —— V2 基础表面基元。
 * 消费 elevation 语义 token（base/raised/floating/inset 四级），
 * 禁止调用方传 hex / 自造阴影。所有视觉值来自 var(--*)。
 */

import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';
import { BORDER_RADIUS, type ElevationLevel } from '@/lib/design-tokens';

export type SurfaceRadius = 'sm' | 'md' | 'lg' | 'xl' | 'none';

const ELEVATION_BG: Record<ElevationLevel, string> = {
  base: 'var(--elevation-base)',
  raised: 'var(--elevation-raised)',
  floating: 'var(--elevation-floating)',
  inset: 'var(--elevation-inset)',
};

const ELEVATION_SHADOW: Record<ElevationLevel, string> = {
  base: 'var(--elev-shadow-base)',
  raised: 'var(--elev-shadow-raised)',
  floating: 'var(--elev-shadow-floating)',
  inset: 'var(--elev-shadow-inset)',
};

const RADIUS_MAP: Record<SurfaceRadius, number> = {
  sm: BORDER_RADIUS.sm,
  md: BORDER_RADIUS.md,
  lg: BORDER_RADIUS.lg,
  xl: BORDER_RADIUS.xl,
  none: 0,
};

export interface SurfaceProps extends HTMLAttributes<HTMLDivElement> {
  elevation?: ElevationLevel;
  radius?: SurfaceRadius;
  /** 是否使用 1px 语义边框（floating/raised 常用）。 */
  bordered?: boolean;
  /** 覆盖最终 boxShadow（仍只能引用 token 变量）。 */
  shadow?: string;
  children?: ReactNode;
  style?: CSSProperties;
}

export function Surface({
  elevation = 'raised',
  radius = 'lg',
  bordered = false,
  shadow,
  style,
  children,
  ...rest
}: SurfaceProps) {
  return (
    <div
      {...rest}
      style={{
        position: 'relative',
        background: ELEVATION_BG[elevation],
        boxShadow: shadow ?? ELEVATION_SHADOW[elevation],
        borderRadius: RADIUS_MAP[radius],
        border: bordered ? '1px solid var(--border-subtle)' : undefined,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

export default Surface;
