'use client';

import { BORDER_RADIUS, FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import type { ProviderRoutingSettings } from '@/types/provider-routing';

interface RoutingPanelProps {
  locale: Locale;
  settings: ProviderRoutingSettings | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  onSave: (settings: ProviderRoutingSettings) => Promise<void>;
  onRetry: () => void;
}

/**
 * 问题8：旧"路由模式"整体退役后，本面板只保留「路由」开关。
 *
 * 新语义：路由开关 = 是否由 Daemon 为当前上游在 Chat Completions /
 * Responses / Anthropic Messages 之间自动选择协议（ProtocolResolver）。
 * 开关关闭 = 沿用供应商显式协议（既有配置可运行）。
 *
 * 已退役（不再提供 UI / 公开类型 / 执行入口）：failover 路由绑定、
 * loopback 本地服务、request rectifier、global outbound proxy、手工
 * route binding、Sub2API 账号池路由绑定。旧 DB 表/列保留 inert。
 */
export function RoutingPanel({ locale, settings, loading, saving, error, onSave, onRetry }: RoutingPanelProps) {
  if (loading) return <div style={stateStyle}>{t(locale, 'common.loading')}</div>;
  if (error || !settings) {
    return (
      <div role="alert" style={errorStyle}>
        <span>{error ?? t(locale, 'settings.routingUnavailable')}</span>
        <button type="button" className="btn" onClick={onRetry}>{t(locale, 'common.retry')}</button>
      </div>
    );
  }
  return (
    <section aria-busy={saving}>
      <div style={masterStyle}>
        <div>
          <h3 style={titleStyle}>{t(locale, 'settings.providerRoutingTitle')}</h3>
          <p style={descriptionStyle}>{t(locale, 'settings.providerRoutingDesc')}</p>
        </div>
        <label style={toggleStyle}>
          <span>{t(locale, 'settings.routingEnabled')}</span>
          <input
            type="checkbox"
            checked={settings.enabled}
            disabled={saving}
            onChange={(event) => void onSave({ ...settings, enabled: event.target.checked })}
            style={{ accentColor: 'var(--primary)', width: 16, height: 16 }}
          />
        </label>
      </div>
      <p style={hintStyle}>{t(locale, 'settings.routingAutoProtocolHint')}</p>
    </section>
  );
}

const masterStyle: React.CSSProperties = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.lg,
  marginBottom: SPACING.lg, padding: SPACING.xl, border: '1px solid var(--border)',
  borderRadius: BORDER_RADIUS.lg, background: 'var(--surface)',
};
const titleStyle: React.CSSProperties = { margin: 0, fontSize: FONT_SIZE.lg, color: 'var(--text)' };
const descriptionStyle: React.CSSProperties = { display: 'block', margin: '4px 0 0', color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
const toggleStyle: React.CSSProperties = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.md,
  color: 'var(--text)', fontSize: FONT_SIZE.sm, cursor: 'pointer',
};
const hintStyle: React.CSSProperties = { margin: 0, color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
const stateStyle: React.CSSProperties = { padding: SPACING.xl, color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm };
const errorStyle: React.CSSProperties = {
  display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.md,
  padding: SPACING.lg, border: '1px solid var(--danger)', borderRadius: BORDER_RADIUS.md,
  color: 'var(--danger)', fontSize: FONT_SIZE.sm,
};
