'use client';

import { useEffect, useState } from 'react';
import { BORDER_RADIUS, FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import type { ProviderSummary } from '@/types/provider';

/** 子代理允许的供应商（沿用既有 subagent_allowed_providers 设置，非路由模式）。 */
export function SubagentProviderSettings({
  locale,
  providers,
}: {
  locale: Locale;
  providers: ProviderSummary[];
}) {
  const [allowedProviders, setAllowedProviders] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    void (async () => {
      try {
        const api = (window as unknown as { nativesAPI?: { db?: { get: (k: string) => Promise<unknown> }; settings?: { get: (k: string) => Promise<unknown> } } }).nativesAPI;
        const raw = (await api?.settings?.get?.('subagent_allowed_providers')) ?? (await api?.db?.get?.('subagent_allowed_providers'));
        if (typeof raw === 'string') {
          const parsed = JSON.parse(raw);
          if (Array.isArray(parsed)) setAllowedProviders(parsed.map(String));
        } else if (Array.isArray(raw)) {
          setAllowedProviders(raw.map(String));
        }
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const toggleProvider = async (id: string) => {
    const next = allowedProviders.includes(id)
      ? allowedProviders.filter((p) => p !== id)
      : [...allowedProviders, id];
    setAllowedProviders(next);
    const api = (window as unknown as { nativesAPI?: { db?: { set: (k: string, v: unknown) => Promise<unknown> }; settings?: { set: (k: string, v: unknown) => Promise<unknown> } } }).nativesAPI;
    if (api?.settings?.set) {
      await api.settings.set('subagent_allowed_providers', next);
    } else if (api?.db?.set) {
      await api.db.set('subagent_allowed_providers', JSON.stringify(next));
    }
  };

  if (loading) return <div style={hintStyle}>{t(locale, 'common.loading')}</div>;

  return (
    <div style={formStyle}>
      <p style={hintStyle}>
        {t(locale, 'routingPanel.subagentHint')}
      </p>
      <div style={{ display: 'grid', gap: SPACING.xs }}>
        {providers.map((p) => {
          const checked = allowedProviders.includes(p.id);
          const hasKeys = p.keys.length > 0;
          return (
            <label
              key={p.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: SPACING.md,
                padding: '6px 10px',
                border: '1px solid var(--border)',
                borderRadius: BORDER_RADIUS.sm,
                background: 'var(--surface)',
                cursor: 'pointer',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => void toggleProvider(p.id)}
                  style={{ accentColor: 'var(--primary)', width: 16, height: 16 }}
                />
                <span style={{ fontWeight: 500, fontSize: FONT_SIZE.sm }}>{p.displayName}</span>
              </div>
              <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)' }}>
                {hasKeys
                  ? t(locale, 'routingPanel.keyCount', { count: p.keys.length })
                  : t(locale, 'routingPanel.noKeys')}
              </span>
            </label>
          );
        })}
      </div>
    </div>
  );
}

const formStyle: React.CSSProperties = { display: 'grid', gap: SPACING.md, maxWidth: 520 };
const hintStyle: React.CSSProperties = { margin: 0, color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
