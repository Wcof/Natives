'use client';

/**
 * ChartFrame —— 图表容器框架。
 * 消费 chart 语义 token（--chart-grid / --chart-axis / --chart-area-fill），
 * 内置 loading（ChartSkeleton）/ empty / error 三态。
 * 浅色主题的 area fill ≈15%→0% 渐变由 --chart-area-fill 提供（法则 4）。
 */

import type { ReactNode } from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { Panel, type PanelProps } from './Panel';
import { Skeleton } from './Skeleton';

export interface ChartFrameProps extends Omit<PanelProps, 'children' | 'title'> {
  title?: ReactNode;
  actions?: ReactNode;
  loading?: boolean;
  empty?: boolean;
  emptyLabel?: ReactNode;
  error?: boolean;
  errorLabel?: ReactNode;
  onRetry?: () => void;
  height?: number | string;
  children?: ReactNode;
}

export function ChartFrame({
  title,
  actions,
  loading,
  empty,
  emptyLabel,
  error,
  errorLabel,
  onRetry,
  height = 220,
  children,
  ...panelProps
}: ChartFrameProps) {
  return (
    <Panel
      {...panelProps}
      title={title}
      actions={actions}
      bodyPadding={SPACING.md}
    >
      <div style={{ height, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
        {loading ? (
          <ChartSkeleton />
        ) : error ? (
          <div style={{ flex: 1, display: 'grid', placeItems: 'center', textAlign: 'center' }}>
            <div>
              <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', marginBottom: SPACING.sm }}>
                {errorLabel ?? 'Failed to load chart'}
              </div>
              {onRetry && (
                <button className="btn" onClick={onRetry} style={{ fontSize: FONT_SIZE.xs }}>
                  Retry
                </button>
              )}
            </div>
          </div>
        ) : empty ? (
          <div style={{ flex: 1, display: 'grid', placeItems: 'center', color: 'var(--text-tertiary)', fontSize: FONT_SIZE.sm }}>
            {emptyLabel ?? 'No data'}
          </div>
        ) : (
          <div style={{ flex: 1, minHeight: 0, position: 'relative' }}>{children}</div>
        )}
      </div>
    </Panel>
  );
}

/** 图表骨架：模拟坐标网格 + 数据条/折线。 */
export function ChartSkeleton({ lines = 5 }: { lines?: number }) {
  return (
    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: SPACING.sm, padding: SPACING.sm }}>
      {/* 模拟横向网格线 */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm, flex: 1 }}>
        {Array.from({ length: lines }, (_, i) => (
          <div
            key={i}
            style={{
              flex: 1,
              borderBottom: '1px dashed var(--chart-grid)',
            }}
          />
        ))}
      </div>
      {/* 模拟柱状条 */}
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: 6, height: 48 }}>
        {[0.35, 0.6, 0.45, 0.8, 0.55, 0.95, 0.7].map((h, i) => (
          <div key={i} className="anim-shimmer" style={{ flex: 1, height: `${Math.round(h * 48)}px`, borderRadius: 4 }} />
        ))}
      </div>
    </div>
  );
}

export default ChartFrame;
