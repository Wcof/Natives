'use client';

/**
 * MetricBlock —— 标签 + 数值 + 可选 Delta 的指标块。
 * 颜色只消费语义 token：delta 使用 danger/success（明确语义）或中性灰；
 * 普通数值使用 --text / --text-secondary。
 */

import type { ReactNode } from 'react';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';

export interface MetricBlockProps {
  label: ReactNode;
  value: ReactNode;
  /** 相比上一期的百分比变化（如 -0.8 表示 -0.8%）。 */
  delta?: number | null;
  /** 明确语义方向（lower-better 与 higher-better 反转配色）。 */
  deltaDirection?: 'lower-better' | 'higher-better';
  mono?: boolean;
  size?: 'sm' | 'md' | 'lg';
  /** 额外说明（例如覆盖率 %）。 */
  hint?: ReactNode;
}

const VALUE_SIZE: Record<NonNullable<MetricBlockProps['size']>, string> = {
  sm: '13px',
  md: '18px',
  lg: '24px',
};

export function MetricBlock({
  label,
  value,
  delta,
  deltaDirection = 'higher-better',
  mono = false,
  size = 'md',
  hint,
}: MetricBlockProps) {
  let deltaNode: ReactNode = null;
  if (delta !== null && delta !== undefined && Number.isFinite(delta) && Math.abs(delta) >= 0.5) {
    const isUp = delta > 0;
    const isGood = deltaDirection === 'higher-better' ? isUp : !isUp;
    deltaNode = (
      <span
        style={{
          fontSize: '10px',
          fontWeight: 600,
          padding: '2px 6px',
          borderRadius: 4,
          background: isGood ? 'var(--success-soft)' : 'var(--danger-soft)',
          color: isGood ? 'var(--success)' : 'var(--danger)',
          marginLeft: 6,
        }}
      >
        {isUp ? '+' : ''}
        {delta.toFixed(1)}%
      </span>
    );
  } else if (delta !== null && delta !== undefined) {
    deltaNode = (
      <span style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginLeft: 6 }}>—</span>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
      <div
        style={{
          fontSize: FONT_SIZE.xs,
          color: 'var(--text-secondary)',
          display: 'flex',
          alignItems: 'center',
          gap: SPACING.xs,
          whiteSpace: 'nowrap',
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: VALUE_SIZE[size],
          fontWeight: 700,
          color: 'var(--text)',
          fontFamily: mono ? 'var(--font-mono)' : undefined,
          display: 'flex',
          alignItems: 'center',
          lineHeight: 1.2,
        }}
      >
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {value}
        </span>
        {deltaNode}
      </div>
      {hint && (
        <div
          style={{
            fontSize: FONT_SIZE.micro,
            color: 'var(--text-tertiary)',
            display: 'flex',
            justifyContent: 'space-between',
          }}
        >
          {hint}
        </div>
      )}
    </div>
  );
}

export default MetricBlock;
