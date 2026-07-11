'use client';

import { useMemo, useState } from 'react';
import { Check, CircleAlert, Eye, EyeOff, KeyRound, Plus, RefreshCw, Search, Server, Trash2, Wifi } from 'lucide-react';
import { BORDER_RADIUS, FONT_SIZE, SPACING, TRANSITION } from '@/lib/design-tokens';
import { t } from '@/i18n';
import type { Locale } from '@/i18n';
import type { UserProvider } from '@/types/provider';

interface ProviderManagementProps {
  locale: Locale;
  providers: UserProvider[];
  loading: boolean;
  defaultProviderId: string;
  defaultProviderKeyId: string;
  defaultModelId: string;
  newKeyValues: Record<string, string>;
  testingKeyId: string | null;
  /** Models discovered via daemon, keyed by provider ID */
  discoveredModels?: Record<string, Array<{
    id: string;
    displayName?: string;
    contextWindow?: number;
    maxOutput?: number;
    source?: string;
    capabilities?: Record<string, boolean>;
  }>>;
  onAdd: () => void;
  onApply: (providerId: string, keyId?: string | null, modelId?: string) => void | Promise<void>;
  onModelChange: (providerId: string, modelId: string) => void | Promise<void>;
  onKeyChange: (providerId: string, keyId: string) => void | Promise<void>;
  onNewKeyChange: (providerId: string, value: string) => void;
  onAddKey: (providerId: string) => void | Promise<void>;
  onTestKey: (providerId: string, keyId: string) => void | Promise<void>;
  onDeleteKey: (providerId: string, keyId: string) => void;
  onDeleteProvider: (providerId: string) => void;
  onDiscoverModels?: (providerId: string) => void | Promise<void>;
}

function statusFor(provider: UserProvider) {
  const keys = provider.keys ?? [];
  if (keys.some((key) => key.status === 'valid')) return { label: 'settings.providerStatusReady', color: 'var(--success)' };
  if (keys.some((key) => key.status === 'invalid')) return { label: 'settings.providerStatusInvalid', color: 'var(--danger)' };
  if (keys.length > 0) return { label: 'settings.providerStatusUnknown', color: 'var(--warning)' };
  return { label: 'settings.providerStatusEmpty', color: 'var(--text-disabled)' };
}

export default function ProviderManagement({
  locale, providers, loading, defaultProviderId, defaultProviderKeyId, defaultModelId,
  newKeyValues, testingKeyId, onAdd, onApply, onModelChange, onKeyChange,
  onNewKeyChange, onAddKey, onTestKey, onDeleteKey, onDeleteProvider,
}: ProviderManagementProps) {
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(defaultProviderId || providers[0]?.id || null);
  const [visibleKeys, setVisibleKeys] = useState<Record<string, boolean>>({});

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return providers;
    return providers.filter((provider) => `${provider.name} ${provider.baseUrl}`.toLowerCase().includes(needle));
  }, [providers, query]);
  const selected = providers.find((provider) => provider.id === selectedId) ?? filtered[0] ?? providers[0];
  const selectedStatus = selected ? statusFor(selected) : null;
  const keys = selected?.keys ?? [];
  const activeKeyId = selected?.id === defaultProviderId ? defaultProviderKeyId : '';

  if (loading) {
    return <div style={emptyStyle}>{t(locale, 'settings.providersLoading')}</div>;
  }

  return (
    <section>
      <div style={headerStyle}>
        <div>
          <h2 style={{ ...sectionTitleStyle, marginBottom: 4 }}>{t(locale, 'settings.providers')}</h2>
          <p style={descriptionStyle}>{t(locale, 'settings.providerManagementDescription')}</p>
        </div>
        <button type="button" className="btn btn-primary" onClick={onAdd} style={buttonStyle}>
          <Plus size={15} /> {t(locale, 'settings.addProvider')}
        </button>
      </div>

      {providers.length === 0 ? (
        <div style={emptyStyle}>
          <Server size={24} style={{ color: 'var(--text-disabled)', marginBottom: SPACING.sm }} />
          <div style={{ color: 'var(--text)', marginBottom: SPACING.xs }}>{t(locale, 'settings.noProviders')}</div>
          <div style={descriptionStyle}>{t(locale, 'settings.providerEmptyDescription')}</div>
        </div>
      ) : (
        <div style={layoutStyle}>
          <aside style={listStyle}>
            <div style={searchStyle}>
              <Search size={14} style={{ color: 'var(--text-disabled)', flexShrink: 0 }} />
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t(locale, 'settings.searchProvider')} style={searchInputStyle} />
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginTop: SPACING.sm }}>
              {filtered.map((provider) => {
                const status = statusFor(provider);
                const isSelected = selected?.id === provider.id;
                const isApplied = provider.id === defaultProviderId;
                return (
                  <button type="button" key={provider.id} onClick={() => setSelectedId(provider.id)} style={{ ...providerItemStyle, ...(isSelected ? selectedItemStyle : {}) }}>
                    <span style={{ ...statusDotStyle, background: status.color }} />
                    <span style={{ minWidth: 0, flex: 1, textAlign: 'left' }}>
                      <span style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text)', fontWeight: 600, fontSize: FONT_SIZE.sm }}>
                        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{provider.name}</span>
                        {isApplied && <Check size={13} style={{ color: 'var(--primary)', flexShrink: 0 }} />}
                      </span>
                      <span style={metaStyle}>{provider.keys?.length ?? 0} {t(locale, 'settings.providerKeysLabel')}</span>
                    </span>
                  </button>
                );
              })}
            </div>
          </aside>

          {selected && (
            <div style={detailStyle}>
              <div style={detailHeaderStyle}>
                <div style={{ minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <h3 style={{ margin: 0, fontSize: FONT_SIZE.lg, color: 'var(--text)' }}>{selected.name}</h3>
                    {selected.id === defaultProviderId && <span style={appliedBadgeStyle}>{t(locale, 'settings.applied')}</span>}
                    {selectedStatus && <span style={{ ...statusBadgeStyle, color: selectedStatus.color }}><span style={{ ...statusDotStyle, background: selectedStatus.color }} />{t(locale, selectedStatus.label)}</span>}
                  </div>
                  <div style={urlStyle}>{selected.baseUrl || selected.websiteUrl}</div>
                </div>
                <button type="button" className="btn-ghost" onClick={() => onDeleteProvider(selected.id)} title={t(locale, 'settings.deleteProvider')} style={iconButtonStyle}>
                  <Trash2 size={15} />
                </button>
              </div>

              <div style={panelSectionStyle}>
                <div style={sectionLabelStyle}>{t(locale, 'settings.providerApplication')}</div>
                <div style={applicationGridStyle}>
                  <label style={fieldStyle}><span>{t(locale, 'settings.defaultKey')}</span>
                    <select value={activeKeyId} onChange={(event) => void onKeyChange(selected.id, event.target.value)} style={controlStyle}>
                      <option value="">{t(locale, 'settings.selectKey')}</option>
                      {keys.map((key) => <option key={key.id} value={key.id}>{key.label} · {key.maskedKey}</option>)}
                    </select>
                  </label>
                  <label style={fieldStyle}><span>{t(locale, 'settings.defaultModel')}</span>
                    <input value={selected.id === defaultProviderId ? defaultModelId : ''} onChange={(event) => onModelChange(selected.id, event.target.value)} onBlur={(event) => void onApply(selected.id, activeKeyId || null, event.target.value.trim())} placeholder="model-id" style={controlStyle} />
                  </label>
                </div>
                <button type="button" className="btn btn-primary" onClick={() => void onApply(selected.id, activeKeyId || keys[0]?.id || null, defaultModelId.trim() || undefined)} disabled={keys.length === 0} style={{ ...buttonStyle, marginTop: SPACING.md }}>
                  <Check size={15} /> {t(locale, 'settings.applyProvider')}
                </button>
              </div>

              <div style={panelSectionStyle}>
                <div style={sectionLabelStyle}>{t(locale, 'settings.providerKeysTitle')}</div>
                {keys.length === 0 && <div style={inlineEmptyStyle}>{t(locale, 'settings.providerNoKeys')}</div>}
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {keys.map((key) => {
                    const visible = visibleKeys[key.id] === true;
                    const keyStatus = key.status === 'valid' ? 'var(--success)' : key.status === 'invalid' ? 'var(--danger)' : 'var(--warning)';
                    return (
                      <div key={key.id} style={keyRowStyle}>
                        <KeyRound size={15} style={{ color: 'var(--text-disabled)', flexShrink: 0 }} />
                        <span style={{ minWidth: 76, color: 'var(--text)', fontSize: FONT_SIZE.sm }}>{key.label}</span>
                        <code style={{ flex: 1, color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs, overflow: 'hidden', textOverflow: 'ellipsis' }}>{visible ? key.maskedKey : '••••••••••••'}</code>
                        <span title={key.lastError || t(locale, key.status === 'valid' ? 'settings.providerKeyValid' : 'settings.providerKeyNotTested')} style={{ ...statusDotStyle, background: keyStatus }} />
                        <button type="button" className="btn-ghost" onClick={() => setVisibleKeys((state) => ({ ...state, [key.id]: !visible }))} title={t(locale, visible ? 'settings.hideKey' : 'settings.showKey')} style={iconButtonStyle}>{visible ? <EyeOff size={14} /> : <Eye size={14} />}</button>
                        <button type="button" className="btn-ghost" onClick={() => void onTestKey(selected.id, key.id)} disabled={testingKeyId === key.id} title={t(locale, 'settings.testProvider')} style={iconButtonStyle}>{testingKeyId === key.id ? <RefreshCw size={14} className="spin" /> : <Wifi size={14} />}</button>
                        <button type="button" className="btn-ghost" onClick={() => onDeleteKey(selected.id, key.id)} title={t(locale, 'settings.deleteProviderKey')} style={{ ...iconButtonStyle, color: 'var(--danger)' }}><Trash2 size={14} /></button>
                      </div>
                    );
                  })}
                </div>
                <div style={{ display: 'flex', gap: SPACING.sm, marginTop: SPACING.md }}>
                  <input type="password" value={newKeyValues[selected.id] || ''} onChange={(event) => onNewKeyChange(selected.id, event.target.value)} placeholder={t(locale, 'settings.addProviderKeyPlaceholder')} style={{ ...controlStyle, flex: 1 }} />
                  <button type="button" className="btn" onClick={() => void onAddKey(selected.id)} disabled={!newKeyValues[selected.id]?.trim()} style={buttonStyle}><Plus size={14} /> {t(locale, 'settings.addProviderKey')}</button>
                </div>
              </div>

              {selectedStatus?.label === 'settings.providerStatusInvalid' && <div style={errorStyle}><CircleAlert size={15} />{t(locale, 'settings.providerFixKey')}</div>}
            </div>
          )}
        </div>
      )}
    </section>
  );
}

const headerStyle: React.CSSProperties = { display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: SPACING.lg, marginBottom: SPACING.lg };
const sectionTitleStyle: React.CSSProperties = { fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)', margin: 0 };
const descriptionStyle: React.CSSProperties = { margin: 0, color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs, lineHeight: 1.5 };
const buttonStyle: React.CSSProperties = { display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6, whiteSpace: 'nowrap' };
const emptyStyle: React.CSSProperties = { minHeight: 190, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', border: '1px dashed var(--border)', borderRadius: BORDER_RADIUS.md, color: 'var(--text-secondary)', textAlign: 'center', padding: SPACING.xxl };
const layoutStyle: React.CSSProperties = { display: 'grid', gridTemplateColumns: 'minmax(190px, 0.34fr) minmax(0, 1fr)', border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.md, overflow: 'hidden', minHeight: 430 };
const listStyle: React.CSSProperties = { background: 'var(--surface-hover)', borderRight: '1px solid var(--border)', padding: SPACING.md, minWidth: 0 };
const detailStyle: React.CSSProperties = { background: 'var(--surface)', minWidth: 0 };
const searchStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 8, height: 34, padding: '0 9px', border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, background: 'var(--surface)' };
const searchInputStyle: React.CSSProperties = { flex: 1, minWidth: 0, border: 0, outline: 0, background: 'transparent', color: 'var(--text)', fontSize: FONT_SIZE.xs };
const providerItemStyle: React.CSSProperties = { width: '100%', display: 'flex', alignItems: 'center', gap: 9, border: '1px solid transparent', borderRadius: BORDER_RADIUS.sm, background: 'transparent', padding: '10px 9px', cursor: 'pointer', transition: `background ${TRANSITION.normal}, border-color ${TRANSITION.normal}` };
const selectedItemStyle: React.CSSProperties = { background: 'var(--primary-soft)', borderColor: 'var(--primary)' };
const statusDotStyle: React.CSSProperties = { width: 7, height: 7, borderRadius: '50%', display: 'inline-block', flexShrink: 0 };
const metaStyle: React.CSSProperties = { display: 'block', marginTop: 3, color: 'var(--text-disabled)', fontSize: FONT_SIZE.micro };
const detailHeaderStyle: React.CSSProperties = { display: 'flex', justifyContent: 'space-between', gap: SPACING.md, padding: SPACING.xl, borderBottom: '1px solid var(--border)' };
const urlStyle: React.CSSProperties = { marginTop: 6, color: 'var(--text-disabled)', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.micro, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' };
const appliedBadgeStyle: React.CSSProperties = { padding: '2px 7px', borderRadius: BORDER_RADIUS.xs, background: 'var(--primary-soft)', color: 'var(--primary)', fontSize: FONT_SIZE.micro, fontWeight: 600 };
const statusBadgeStyle: React.CSSProperties = { display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: FONT_SIZE.micro };
const iconButtonStyle: React.CSSProperties = { display: 'inline-flex', alignItems: 'center', justifyContent: 'center', padding: 6, minWidth: 28, minHeight: 28 };
const panelSectionStyle: React.CSSProperties = { padding: `${SPACING.lg}px ${SPACING.xl}px`, borderBottom: '1px solid var(--border)' };
const sectionLabelStyle: React.CSSProperties = { color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs, fontWeight: 600, marginBottom: SPACING.sm };
const applicationGridStyle: React.CSSProperties = { display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: SPACING.md };
const fieldStyle: React.CSSProperties = { display: 'flex', flexDirection: 'column', gap: 6, color: 'var(--text-secondary)', fontSize: FONT_SIZE.xs };
const controlStyle: React.CSSProperties = { minWidth: 0, height: 36, padding: '0 10px', border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, background: 'var(--surface-hover)', color: 'var(--text)', fontSize: FONT_SIZE.xs, outline: 0 };
const keyRowStyle: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 8, minHeight: 38, padding: '4px 6px 4px 9px', border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.sm, background: 'var(--surface-hover)' };
const inlineEmptyStyle: React.CSSProperties = { color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs, padding: `${SPACING.sm}px 0` };
const errorStyle: React.CSSProperties = { display: 'flex', gap: 8, alignItems: 'center', padding: `${SPACING.sm}px ${SPACING.xl}px`, color: 'var(--danger)', background: 'color-mix(in srgb, var(--danger) 10%, transparent)', fontSize: FONT_SIZE.xs };
