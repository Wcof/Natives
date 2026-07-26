'use client';

import { useEffect, useState } from 'react';
import { Bot, ChevronDown, Globe2, HeartPulse, Route, Sparkles } from 'lucide-react';
import { BORDER_RADIUS, FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import type { ProviderRoutingSettings } from '@/types/provider-routing';
import type { ProviderRouteBinding } from '@/types/provider-routing';
import type { ProviderSummary } from '@/types/provider';

interface RoutingPanelProps {
  locale: Locale;
  settings: ProviderRoutingSettings | null;
  providers: ProviderSummary[];
  bindings: ProviderRouteBinding[];
  loading: boolean;
  saving: boolean;
  error: string | null;
  onSave: (settings: ProviderRoutingSettings) => Promise<void>;
  onSaveBindings: (bindings: ProviderRouteBinding[]) => Promise<void>;
  onRetry: () => void;
}

export function RoutingPanel({ locale, providers, bindings, settings, loading, saving, error, onSave, onSaveBindings, onRetry }: RoutingPanelProps) {
  const [expanded, setExpanded] = useState<string | null>('subagent');
  if (loading) return <div style={stateStyle}>{t(locale, 'common.loading')}</div>;
  if (error || !settings) return <div role="alert" style={errorStyle}><span>{error ?? t(locale, 'settings.routingUnavailable')}</span><button type="button" className="btn" onClick={onRetry}>{t(locale, 'common.retry')}</button></div>;
  const update = (patch: Partial<ProviderRoutingSettings>) => void onSave({ ...settings, ...patch });
  const zh = locale.startsWith('zh');
  const sections = [
    { id: 'subagent', icon: <Bot size={20} />, title: zh ? '子智能体供应商' : 'Subagent Providers', description: zh ? '配置允许子智能体使用的供应商范围' : 'Select allowed providers for subagents', body: <SubagentProviderSettings locale={locale} providers={providers} /> },
    { id: 'local', icon: <Route size={20} />, title: t(locale, 'settings.localRouting'), description: t(locale, 'settings.localRoutingDesc'), body: <LocalRouting locale={locale} settings={settings} onSave={update} /> },
    { id: 'failover', icon: <HeartPulse size={20} />, title: t(locale, 'settings.autoFailover'), description: t(locale, 'settings.autoFailoverDesc'), body: <FailoverSettings locale={locale} providers={providers} bindings={bindings} onSave={onSaveBindings} /> },
    { id: 'rectifier', icon: <Sparkles size={20} />, title: t(locale, 'settings.requestRectifier'), description: t(locale, 'settings.requestRectifierDesc'), body: <ToggleRow locale={locale} labelKey="settings.requestRectifierEnabled" checked={settings.rectifierEnabled} onChange={(checked) => update({ rectifierEnabled: checked })} /> },
    { id: 'proxy', icon: <Globe2 size={20} />, title: t(locale, 'settings.globalOutboundProxy'), description: t(locale, 'settings.globalOutboundProxyDesc'), body: <ProxySettings locale={locale} settings={settings} onSave={update} /> },
  ];
  return (
    <section aria-busy={saving}>
      <div style={masterStyle}>
        <div><h3 style={titleStyle}>{t(locale, 'settings.providerRoutingTitle')}</h3><p style={descriptionStyle}>{t(locale, 'settings.providerRoutingDesc')}</p></div>
        <ToggleRow locale={locale} labelKey="settings.routingEnabled" checked={settings.enabled} onChange={(checked) => update({ enabled: checked })} disabled={saving} />
      </div>
      <div style={{ display: 'grid', gap: SPACING.md, opacity: settings.enabled ? 1 : 0.62 }}>
        {sections.map((section) => {
          const open = expanded === section.id;
          return <div key={section.id} style={accordionStyle}>
            <button type="button" aria-expanded={open} onClick={() => setExpanded(open ? null : section.id)} style={accordionHeadStyle}>
              <span style={iconStyle}>{section.icon}</span><span style={{ flex: 1, textAlign: 'left' }}><strong style={{ display: 'block', color: 'var(--text)', fontSize: FONT_SIZE.md }}>{section.title}</strong><span style={descriptionStyle}>{section.description}</span></span><ChevronDown size={18} style={{ transform: open ? 'rotate(180deg)' : 'none', transition: 'transform 150ms ease' }} />
            </button>
            {open && <div style={accordionBodyStyle}>{section.body}</div>}
          </div>;
        })}
      </div>
    </section>
  );
}

function FailoverSettings({ locale, providers, bindings, onSave }: { locale: Locale; providers: ProviderSummary[]; bindings: ProviderRouteBinding[]; onSave: (bindings: ProviderRouteBinding[]) => Promise<void> }) {
  const [providerId, setProviderId] = useState(providers[0]?.id ?? '');
  const [modelId, setModelId] = useState('');
  const [keyId, setKeyId] = useState('');
  const provider = providers.find((item) => item.id === providerId);
  useEffect(() => setKeyId(provider?.keys[0]?.id ?? ''), [provider?.id]);
  const add = () => {
    if (!provider || !modelId.trim()) return;
    const credential = provider.providerType === 'sub2api' ? { kind: 'sub2api_pool' as const } : keyId ? { kind: 'api_key' as const, keyId } : null;
    if (!credential) return;
    void onSave([...bindings, { id: crypto.randomUUID(), providerId: provider.id, modelId: modelId.trim(), credential, priority: bindings.length + 1, enabled: true }]);
    setModelId('');
  };
  return <div style={formStyle}>
    <div style={bindingListStyle}>{bindings.length === 0 ? <span style={hintStyle}>{t(locale, 'settings.routeBindingsEmpty')}</span> : bindings.map((binding, index) => <div key={binding.id} style={bindingRowStyle}><input aria-label={binding.modelId} type="checkbox" checked={binding.enabled} onChange={(event) => void onSave(bindings.map((item) => item.id === binding.id ? { ...item, enabled: event.target.checked } : item))} /><span>{providers.find((item) => item.id === binding.providerId)?.displayName ?? binding.providerId}</span><code>{binding.modelId}</code><button type="button" className="btn-ghost" disabled={index === 0} onClick={() => void onSave(bindings.map((item, itemIndex) => itemIndex === index ? bindings[index - 1]! : itemIndex === index - 1 ? bindings[index]! : item))}>↑</button><button type="button" className="btn-ghost" disabled={index === bindings.length - 1} onClick={() => void onSave(bindings.map((item, itemIndex) => itemIndex === index ? bindings[index + 1]! : itemIndex === index + 1 ? bindings[index]! : item))}>↓</button><button type="button" className="btn-ghost" onClick={() => void onSave(bindings.filter((item) => item.id !== binding.id))}>{t(locale, 'common.remove')}</button></div>)}</div>
    <div style={bindingEditorStyle}><select value={providerId} onChange={(event) => setProviderId(event.target.value)} style={inputStyle}>{providers.map((item) => <option key={item.id} value={item.id}>{item.displayName}</option>)}</select>{provider?.providerType === 'sub2api' ? <span style={inputStyle}>{t(locale, 'settings.sub2apiAccountPool')}</span> : <select value={keyId} onChange={(event) => setKeyId(event.target.value)} style={inputStyle}>{provider?.keys.map((key) => <option key={key.id} value={key.id}>{key.label} · {key.maskedKey}</option>)}</select>}<input value={modelId} onChange={(event) => setModelId(event.target.value)} placeholder={t(locale, 'settings.routeBindingModel')} style={inputStyle} /><button type="button" className="btn" disabled={!modelId.trim() || !provider || (provider.providerType !== 'sub2api' && !keyId)} onClick={add}>{t(locale, 'settings.routeBindingAdd')}</button></div>
  </div>;
}

function LocalRouting({ locale, settings, onSave }: { locale: Locale; settings: ProviderRoutingSettings; onSave: (patch: Partial<ProviderRoutingSettings>) => void }) {
  const [issuedToken, setIssuedToken] = useState<string | null>(null);
  const rotate = async () => {
    const token = await window.nativesAPI?.providerRouting?.rotateLoopbackToken?.();
    if (token) setIssuedToken(token);
  };
  return <div style={formStyle}><ToggleRow locale={locale} labelKey="settings.localRoutingEnabled" checked={settings.loopbackEnabled} onChange={(checked) => onSave({ loopbackEnabled: checked })} /><label style={fieldStyle}>{t(locale, 'settings.localRoutingPort')}<input type="number" min={1024} max={65535} value={settings.loopbackPort} disabled={!settings.loopbackEnabled} onChange={(event) => onSave({ loopbackPort: Number(event.target.value) || 15721 })} style={inputStyle} /></label><button type="button" className="btn" onClick={() => void rotate()}>{t(locale, 'settings.localRoutingRotateToken')}</button>{issuedToken && <><label style={fieldStyle}>{t(locale, 'settings.localRoutingTokenOnce')}<input readOnly value={issuedToken} style={inputStyle} /></label><button type="button" className="btn-ghost" onClick={() => void navigator.clipboard.writeText(issuedToken)}>{t(locale, 'common.copy')}</button></>}<p style={hintStyle}>{t(locale, 'settings.localRoutingAuth')}</p></div>;
}

function ProxySettings({ locale, settings, onSave }: { locale: Locale; settings: ProviderRoutingSettings; onSave: (patch: Partial<ProviderRoutingSettings>) => void }) {
  return <div style={formStyle}><ToggleRow locale={locale} labelKey="settings.globalOutboundProxyEnabled" checked={settings.outboundProxyEnabled} onChange={(checked) => onSave({ outboundProxyEnabled: checked })} /><label style={fieldStyle}>{t(locale, 'settings.outboundProxyUrl')}<input value={settings.outboundProxyUrl ?? ''} disabled={!settings.outboundProxyEnabled} placeholder="socks5://127.0.0.1:1080" onChange={(event) => onSave({ outboundProxyUrl: event.target.value || null })} style={inputStyle} /></label><p style={hintStyle}>{t(locale, 'settings.outboundProxyHint')}</p></div>;
}

function ToggleRow({ locale, labelKey, checked, onChange, disabled = false }: { locale: Locale; labelKey: string; checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean }) {
  return <label style={toggleStyle}><span>{t(locale, labelKey)}</span><input type="checkbox" checked={checked} disabled={disabled} onChange={(event) => onChange(event.target.checked)} style={{ accentColor: 'var(--primary)', width: 16, height: 16 }} /></label>;
}

const masterStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.lg, marginBottom: SPACING.lg, padding: SPACING.xl, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.lg, background: 'var(--surface)' };
const titleStyle: React.CSSProperties = { margin: 0, fontSize: FONT_SIZE.lg, color: 'var(--text)' };
const descriptionStyle: React.CSSProperties = { display: 'block', margin: '4px 0 0', color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
const accordionStyle: React.CSSProperties = { border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.lg, overflow: 'hidden', background: 'var(--surface)' };
const accordionHeadStyle: React.CSSProperties = { width: '100%', display: 'flex', alignItems: 'center', gap: SPACING.md, padding: SPACING.xl, border: 0, color: 'var(--text-secondary)', background: 'transparent', cursor: 'pointer' };
const iconStyle: React.CSSProperties = { display: 'inline-flex', color: 'var(--primary)', flexShrink: 0 };
const accordionBodyStyle: React.CSSProperties = { padding: SPACING.xl, borderTop: '1px solid var(--border)', background: 'var(--surface-hover)' };
const formStyle: React.CSSProperties = { display: 'grid', gap: SPACING.md, maxWidth: 520 };
const fieldStyle: React.CSSProperties = { display: 'grid', gap: SPACING.xs, color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs };
const inputStyle: React.CSSProperties = { height: 36, padding: `0 ${SPACING.sm}px`, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, color: 'var(--text)', background: 'var(--surface)', fontSize: FONT_SIZE.sm };
const hintStyle: React.CSSProperties = { margin: 0, color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
const toggleStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.md, color: 'var(--text)', fontSize: FONT_SIZE.sm, cursor: 'pointer' };
const stateStyle: React.CSSProperties = { padding: SPACING.xl, color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm };
const errorStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: SPACING.md, padding: SPACING.lg, border: '1px solid var(--danger)', borderRadius: BORDER_RADIUS.md, color: 'var(--danger)', fontSize: FONT_SIZE.sm };
const bindingListStyle: React.CSSProperties = { display: 'grid', gap: SPACING.xs };
const bindingRowStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: SPACING.sm, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, color: 'var(--text)', fontSize: FONT_SIZE.xs };
const bindingEditorStyle: React.CSSProperties = { display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr) minmax(0, 1fr) auto', gap: SPACING.sm };

export function SubagentProviderSettings({
  locale,
  providers,
}: {
  locale: Locale;
  providers: ProviderSummary[];
}) {
  const zh = locale.startsWith('zh');
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
        {zh
          ? '多选允许子智能体使用的供应商。未勾选时默认仅允许使用主会话的供应商。'
          : 'Select allowed providers for subagents. Defaults to main session provider when unconfigured.'}
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
                  ? zh
                    ? `${p.keys.length} 个有效密钥`
                    : `${p.keys.length} keys`
                  : zh
                    ? '无密钥'
                    : 'No keys'}
              </span>
            </label>
          );
        })}
      </div>
    </div>
  );
}
