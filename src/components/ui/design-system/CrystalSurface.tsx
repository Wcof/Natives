'use client';

/**
 * CrystalSurface —— Liquid Crystal 晶透液态表面（独立浅色规范）。
 * 四条法则：
 *   1. 顶层 Specular Highlight（--highlight-specular）
 *   2. 2-3 层低 alpha 微阴影替代粗边框（--crystal-shadow-1/2/3, --crystal-border）
 *   3. 深石墨灰/中性灰文字层级（文字颜色由调用方消费 --text / --text-secondary）
 *   4. 浅色图表 area fill 15%→0%（见 ChartFrame）
 * blur 仅局部（backdrop 开关），reduced-transparency 时由 CSS 全局禁用。
 */

import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';
import { BORDER_RADIUS } from '@/lib/design-tokens';

export interface CrystalSurfaceProps extends HTMLAttributes<HTMLDivElement> {
  /** 是否启用局部 backdrop blur（默认关；克制使用）。 */
  backdrop?: boolean;
  /** 1-3 层微阴影强度（默认 2）。 */
  shadowDepth?: 1 | 2 | 3;
  radius?: 'sm' | 'md' | 'lg' | 'xl' | 'none';
  children?: ReactNode;
  style?: CSSProperties;
}

const RADIUS_MAP = {
  sm: BORDER_RADIUS.sm,
  md: BORDER_RADIUS.md,
  lg: BORDER_RADIUS.lg,
  xl: BORDER_RADIUS.xl,
  none: 0,
} as const;

const SHADOW_DEPTHS: Record<1 | 2 | 3, string> = {
  1: 'var(--crystal-shadow-1)',
  2: 'var(--crystal-shadow-1), var(--crystal-shadow-2)',
  3: 'var(--crystal-shadow-1), var(--crystal-shadow-2), var(--crystal-shadow-3)',
};

export function CrystalSurface({
  backdrop = false,
  shadowDepth = 2,
  radius = 'lg',
  style,
  children,
  ...rest
}: CrystalSurfaceProps) {
  return (
    <div
      {...rest}
      className={['ds-crystal', rest.className].filter(Boolean).join(' ')}
      style={{
        background: 'var(--surface)',
        border: '1px solid var(--crystal-border)',
        borderRadius: RADIUS_MAP[radius],
        boxShadow: SHADOW_DEPTHS[shadowDepth],
        // 局部 blur：克制、可关闭；reduced-transparency 由 CSS 强制禁用。
        backdropFilter: backdrop ? 'blur(20px) saturate(135%)' : undefined,
        WebkitBackdropFilter: backdrop ? 'blur(20px) saturate(135%)' : undefined,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

export default CrystalSurface;
