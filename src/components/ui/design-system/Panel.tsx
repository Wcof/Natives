'use client';

/**
 * Panel —— 标题栏 + 内容区的标准面板容器。
 * 消费 elevation 语义 token；header 用 hairline（border-subtle）分隔。
 */

import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';
import { SPACING, type ElevationLevel } from '@/lib/design-tokens';
import { Surface, type SurfaceRadius } from './Surface';

export interface PanelProps extends Omit<HTMLAttributes<HTMLDivElement>, 'title'> {
  elevation?: ElevationLevel;
  radius?: SurfaceRadius;
  title?: ReactNode;
  header?: ReactNode;
  actions?: ReactNode;
  /** 内容区 padding（默认 16px）。 */
  bodyPadding?: number | string;
  bodyClassName?: string;
  children?: ReactNode;
  style?: CSSProperties;
}

export function Panel({
  elevation = 'raised',
  radius = 'lg',
  title,
  header,
  actions,
  bodyPadding = SPACING.lg,
  bodyClassName,
  children,
  style,
  ...rest
}: PanelProps) {
  const showHeader = Boolean(header) || Boolean(title) || Boolean(actions);

  return (
    <Surface
      {...rest}
      elevation={elevation}
      radius={radius}
      bordered
      style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden', ...style }}
    >
      {showHeader && (
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: SPACING.sm,
            padding: `${SPACING.sm}px ${SPACING.lg}px`,
            borderBottom: '1px solid var(--border-subtle)',
            minWidth: 0,
          }}
        >
          {header ?? (
            <>
              {title && (
                <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text)', minWidth: 0 }}>
                  {title}
                </div>
              )}
              <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: SPACING.xs }}>
                {actions}
              </div>
            </>
          )}
        </div>
      )}
      <div
        className={bodyClassName}
        style={{
          padding: typeof bodyPadding === 'number' ? `${bodyPadding}px` : bodyPadding,
          minHeight: 0,
          minWidth: 0,
        }}
      >
        {children}
      </div>
    </Surface>
  );
}

export default Panel;
