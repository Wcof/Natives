'use client';

/**
 * WidgetShell（B-018/B-019）—— surfacePolicy/header/edit chrome。
 * 禁止所有 Widget 强制同一种玻璃卡：表面材质由
 *   config.surface（'auto' → def.surfacePolicy.surfaces[0]）决定，
 *   allowBlur / allowGlow 由 definition 的 SurfacePolicy 控制。
 * 不接管 RGL transform：拖拽/缩放由 host 的 RGL 独占，shell 只渲染
 * 静态 chrome（motion 约束 §8）。
 */

import type { CSSProperties, ReactElement, ReactNode } from 'react';
import { GripVertical, X } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { SPACING, type ElevationLevel } from '@/lib/design-tokens';
import {
  MaterialSurface,
  CrystalSurface,
  Surface,
  useSystemReducedTransparency,
} from '@/components/ui/design-system';
import type {
  WidgetInstance,
  WidgetShellProps,
  WidgetSurfaceKind,
} from '@/lib/workspace/widgets';

// Widget chrome 样式（全局一次，幂等）
import '@/app/styles/widgets.css';

function resolveSurfaceKind<TData, TSettings extends Record<string, unknown>>(
  instance: WidgetInstance<TData, TSettings>,
): WidgetSurfaceKind {
  const { def, config } = instance;
  if (config.surface !== 'auto') return config.surface;
  return def.surfacePolicy.surfaces[0] ?? 'plain';
}

function elevationForSize(size: string | undefined): ElevationLevel {
  return size === 'large' ? 'floating' : 'raised';
}

export function WidgetShell<TData, TSettings extends Record<string, unknown>>({
  instance,
  actions,
  children,
  elevation,
  className,
  editing = false,
  onRemove,
}: WidgetShellProps<TData, TSettings>) {
  const locale = useLocale();
  const reducedTransparency = useSystemReducedTransparency();

  const { def, config } = instance;
  const kind = resolveSurfaceKind(instance);
  const policy = def.surfacePolicy;
  const showHeader = def.titleKey != null && def.titleKey.length > 0;

  const shellClassName = ['ws-shell', className, editing ? 'ws-shell--editing' : '']
    .filter(Boolean)
    .join(' ');
  const shellStyle: CSSProperties = {
    height: '100%',
    display: 'flex',
    flexDirection: 'column',
  };

  // ── header + edit chrome ──
  const headerActions: ReactNode[] = [];
  if (editing || Boolean(onRemove)) {
    headerActions.push(
      <button
        key="drag-handle"
        type="button"
        className="ws-drag-handle"
        data-widget-drag-handle
        title={t(locale, 'home.editHint')}
        aria-label={t(locale, 'home.edit')}
      >
        <GripVertical size={13} />
      </button>,
    );
  }
  if (actions) headerActions.push(<div key="actions">{actions}</div>);
  if (onRemove) {
    headerActions.push(
      <button
        key="remove"
        type="button"
        className="ws-icon-button ws-icon-button--danger"
        onClick={onRemove}
        title={t(locale, 'home.removeWidget')}
        aria-label={t(locale, 'home.removeWidget')}
      >
        <X size={13} />
      </button>,
    );
  }

  const title = def.titleKey ? t(locale, def.titleKey) : null;

  const content = (
    <>
      {showHeader && (
        <div
          className="ws-shell-header"
          style={{ padding: `${SPACING.xs}px ${SPACING.sm}px`, gap: SPACING.xs }}
        >
          {title && <span className="ws-shell-header-title">{title}</span>}
          <span className="ws-shell-header-actions">{headerActions}</span>
        </div>
      )}
      <div className="ws-shell-body">{children}</div>
    </>
  );

  // ── 表面 wrapper（按 surfacePolicy；不默认 card/glass） ──
  let surface: ReactElement;
  if (kind === 'crystal') {
    surface = (
      <CrystalSurface
        backdrop={Boolean(policy.allowBlur) && !reducedTransparency}
        shadowDepth={2}
        radius="lg"
        className={shellClassName}
        style={shellStyle}
      >
        {content}
      </CrystalSurface>
    );
  } else if (kind === 'material') {
    surface = (
      <MaterialSurface
        elevation={elevation ?? elevationForSize(config.size ?? def.size)}
        radius="lg"
        tone="selected"
        active={editing && Boolean(policy.allowGlow)}
        className={shellClassName}
        style={shellStyle}
      >
        {content}
      </MaterialSurface>
    );
  } else {
    // plain / bare：稳定不透明，无边框阴影（低干扰默认）。
    surface = (
      <Surface
        elevation={elevation ?? 'base'}
        radius="lg"
        className={shellClassName}
        style={shellStyle}
      >
        {content}
      </Surface>
    );
  }

  return surface;
}

export default WidgetShell;
