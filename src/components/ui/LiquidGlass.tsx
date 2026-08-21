'use client';

/**
 * LiquidGlass —— V2 版本：CSS-only 局部玻璃，无 WebGL、无全屏 canvas、无物理畸变。
 *
 * 硬边界：blur 局部使用，无全屏 WebGL；glass 强度全部走语义 token
 * （--crystal-border / --crystal-shadow-* / --highlight-specular），
 * 禁止组件内硬编码 hex / rgba。
 *
 * 旧参数（displacementScale / aberrationIntensity / elasticity）为 WebGL 时代
 * 残留，本实现不接受物理畸变，仅映射 blur / saturation 两个克制参数。
 */

import type { CSSProperties, ReactNode } from 'react';
import { BORDER_RADIUS } from '@/lib/design-tokens';

interface LiquidGlassProps {
  isActive: boolean;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  /** 局部磨砂强度 px（默认 20；克制上限 40）。 */
  blurAmount?: number;
  /** 局部饱和度 %（默认 135）。 */
  saturation?: number;
  displacementScale?: number;
  aberrationIntensity?: number;
  elasticity?: number;
}

export default function LiquidGlass({
  isActive,
  children,
  className,
  style,
  blurAmount = 20,
  saturation = 135,
}: LiquidGlassProps) {
  const radius = BORDER_RADIUS.xl;

  if (!isActive) {
    return (
      <div className={className} style={style}>
        {children}
      </div>
    );
  }

  return (
    <div
      className={['ds-crystal', className].filter(Boolean).join(' ')}
      style={{
        position: 'relative',
        background: 'var(--surface)',
        border: '1px solid var(--crystal-border)',
        borderRadius: radius,
        boxShadow:
          'inset 0 1px 0 0 var(--highlight-specular), var(--crystal-shadow-1), var(--crystal-shadow-2)',
        // 局部 blur：reduced-transparency 时由 CSS 全局禁用。
        backdropFilter: `blur(${Math.min(blurAmount, 40)}px) saturate(${saturation}%)`,
        WebkitBackdropFilter: `blur(${Math.min(blurAmount, 40)}px) saturate(${saturation}%)`,
        overflow: 'hidden',
        ...style,
      }}
    >
      {children}
    </div>
  );
}
