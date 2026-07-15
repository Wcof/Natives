'use client';

import { useState, useEffect, useCallback } from 'react';
import { normalizeThemeId, applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { SPACING } from '@/lib/design-tokens';
import { Palette, Globe, Package, RefreshCw, Trash, Loader, RotateCcw } from 'lucide-react';
import RuntimePanel from '@/components/assistant/RuntimePanel';
import ProviderDetail from '@/components/settings/ProviderDetail';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';
import {
  type SettingsSection,
} from './settings-navigation';

// ── Theme skins — only light/dark as supported by the theme engine ──
const THEMES = [
  { id: 'light', labelKey: 'settings.themeJasmine', icon: '☀️' },
  { id: 'dark', labelKey: 'settings.themeTerminal', icon: '🌙' },
];

// ── Local components ──

function SettingsPageHeader({
  title,
  description,
  action,
}: {
  title: string;
  description: string;
  action?: React.ReactNode;
}) {
  return (
    <header className="mb-8 flex items-start justify-between gap-4">
      <div>
        <h2 className="text-xl font-semibold tracking-tight text-[var(--text)]">
          {title}
        </h2>
        <p className="mt-1.5 text-sm text-[var(--text-secondary)]">
          {description}
        </p>
      </div>
      {action}
    </header>
  );
}

function InlineLoadError({
  message,
  onRetry,
  locale,
}: {
  message: string;
  onRetry: () => void;
  locale: Locale;
}) {
  return (
    <div
      role="alert"
      className="flex flex-col items-center gap-3 rounded-lg border border-[var(--danger)] p-6 text-center"
    >
      <p className="text-sm text-[var(--danger)]">{message}</p>
      <button
        type="button"
        className="btn btn-primary inline-flex items-center gap-2 text-xs"
        onClick={onRetry}
      >
        <RotateCcw size={12} />
        {t(locale, 'common.retry')}
      </button>
    </div>
  );
}

export default function SettingsPage({
  activeSection = 'general',
  locale: externalLocale,
}: {
  activeSection?: SettingsSection;
  locale?: Locale;
}) {
  const { toast: globalToast } = useToast();
  const [locale, setLocaleState] = useState<Locale>(externalLocale ?? 'zh');

  // Sync with external locale prop when it changes
  useEffect(() => {
    if (externalLocale && externalLocale !== locale) {
      setLocaleState(externalLocale);
    }
  }, [externalLocale]);

  // ── Theme / locale state ──
  const [currentTheme, setCurrentTheme] = useState('light');
  const [currentLocale, setCurrentLocale] = useState('zh');

  // ── Plugin state ──
  const [plugins, setPlugins] = useState<Array<{ id: string; name: string; version?: string; enabled: boolean; description?: string }>>([]);
  const [pluginLoading, setPluginLoading] = useState(false);
  const [pluginsError, setPluginsError] = useState<string | null>(null);

  // Providers state
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [providersLoading, setProvidersLoading] = useState(false);
  const [providersError, setProvidersError] = useState<string | null>(null);
  const [deleteProviderTarget, setDeleteProviderTarget] = useState<string | null>(null);

  useEffect(() => {
    if (activeSection === 'providers') loadProviders();
    if (activeSection === 'appearance') loadTheme();
    if (activeSection === 'plugins') loadPlugins();
  }, [activeSection]);

  useEffect(() => {
    (async () => {
      try {
        const api = window.nativesAPI;
        if (!api) return;
        const sl = await api.getLocale().catch(() => null);
        if (sl) setLocaleState(sl as Locale);
      } catch { /* noop */ }
    })();
  }, []);

  // ── Theme ──
  async function loadTheme() {
    try {
      const api = window.nativesAPI;
      const saved = await api?.db?.get('theme_id').catch(() => null);
      if (saved && typeof saved === 'string') {
        const normalized = normalizeThemeId(saved);
        setCurrentTheme(normalized);
      }
      const loc = await api?.getLocale?.().catch(() => 'zh');
      if (loc) setCurrentLocale(loc);
    } catch { /* ignore */ }
  }

  async function handleSelectTheme(themeId: string) {
    const normalized = normalizeThemeId(themeId);
    setCurrentTheme(normalized);
    applyTheme(normalized);
    try {
      await window.nativesAPI?.db?.set('theme_id', normalized);
      if ((window.nativesAPI as any)?.theme?.setTheme) await (window.nativesAPI as any).theme.setTheme(normalized);
    } catch { /* persist best-effort */ }
  }

  async function handleLocaleChange(next: string) {
    const nextLocale = next === 'en' ? 'en' : 'zh';
    setCurrentLocale(nextLocale);
    setLocaleState(nextLocale as Locale);
    try {
      await window.nativesAPI?.setLocale?.(nextLocale);
      if ((window.nativesAPI as any)?.locale?.setLocale) await (window.nativesAPI as any).locale.setLocale(nextLocale);
    } catch { /* ignore */ }
  }

  // ── Plugins ──
  async function loadPlugins() {
    setPluginLoading(true);
    setPluginsError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.module?.list) { setPlugins([]); return; }
      const list = await api.module.list() as Array<{ id: string; name?: string; version?: string; enabled: boolean; description?: string }>;
      setPlugins(list.map(p => ({ ...p, name: p.name || p.id })));
    } catch (e) {
      setPluginsError(classifyError(e).userMessage);
      setPlugins([]);
    }
    finally { setPluginLoading(false); }
  }

  async function handleTogglePlugin(id: string, enabled: boolean) {
    try {
      const api = window.nativesAPI;
      if (enabled) await api?.module?.disable?.(id);
      else await api?.module?.enable?.(id);
      await loadPlugins();
    } catch (e) { globalToast(classifyError(e).userMessage, 'error'); }
  }

  async function handleUninstallPlugin(id: string) {
    try {
      const api = window.nativesAPI;
      await api?.module?.uninstall?.(id);
      await loadPlugins();
    } catch (e) { globalToast(classifyError(e).userMessage, 'error'); }
  }

  // ── Provider handlers ──
  async function loadProviders() {
    setProvidersLoading(true);
    setProvidersError(null);
    try {
      const api = window.nativesAPI;
      if (api?.provider?.list) {
        const result = await api.provider.list();
        setProviders(Array.isArray(result) ? result as unknown as ProviderSummary[] : []);
      }
    } catch (e) {
      setProvidersError(classifyError(e).userMessage);
      setProviders([]);
    }
    finally { setProvidersLoading(false); }
  }

  async function handleSaveProvider(data: { presetName: string; name: string; websiteUrl: string; baseUrl: string; keys: { label: string; apiKey: string }[] }) {
    const api = window.nativesAPI;
    if (api?.provider?.create) {
      const initialKey = data.keys[0] ? { label: data.keys[0].label, apiKey: data.keys[0].apiKey } : { label: 'default', apiKey: '' };
      await api.provider.create({ providerType: data.presetName, displayName: data.name, websiteUrl: data.websiteUrl, baseUrl: data.baseUrl, defaultModel: '', initialKey });
    } else throw new Error('Provider API not available');
    globalToast(t(locale, 'settings.providerAdded'), 'success');
    await loadProviders();
  }

  async function handleDeleteProvider(id: string) { const a = window.nativesAPI; if (!a?.provider?.delete) return; await a.provider.delete(id); globalToast(t(locale, 'settings.providerDeleted'), 'success'); await loadProviders(); }
  async function handleSaveDefaults(pid: string, m: string | null) { const a = window.nativesAPI; if (a?.provider?.updateDefaults) { await a.provider.updateDefaults({ providerId: pid, defaultModel: m ?? '' }); await loadProviders(); } else throw new Error('updateDefaults'); }
  async function handleAddKey(pid: string, l: string, k: string) { const a = window.nativesAPI; if (a?.provider?.addKey) await a.provider.addKey({ providerId: pid, label: l, apiKey: k }); else throw new Error('addKey'); await loadProviders(); }
  async function handleTestKey(pid: string, kid: string): Promise<TestKeyResult> { const a = window.nativesAPI; if (a?.provider?.testKey) { const r = await a.provider.testKey({ providerId: pid, keyId: kid }); await loadProviders(); return r as unknown as TestKeyResult; } throw new Error('testKey'); }
  async function handleSetPrimaryKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.setPrimaryKey) { await a.provider.setPrimaryKey({ providerId: pid, keyId: kid }); await loadProviders(); } else throw new Error('setPrimaryKey'); }
  async function handleDeleteKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.deleteKey) await a.provider.deleteKey({ providerId: pid, keyId: kid }); else throw new Error('deleteKey'); await loadProviders(); }

  const cardStyle: React.CSSProperties = { background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: SPACING.md };

  // ── Render sections ──

  function renderGeneral() {
    return (
      <>
        <SettingsPageHeader
          title={t(locale, 'settings.tabGeneral')}
          description={t(locale, 'settings.generalDesc')}
        />
        <div style={cardStyle}>
          <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.md }}>
            <Globe size={16} />
            <h3 style={{ fontSize: 'var(--font-size-md)', fontWeight: 600 }}>{t(locale, 'settings.language')}</h3>
          </div>
          <select
            value={currentLocale}
            onChange={(event) =>
              void handleLocaleChange(event.target.value)
            }
            style={{
              padding: `${SPACING.xs}px ${SPACING.md}px`,
              borderRadius: 'var(--radius-sm)',
              border: '1px solid var(--border)',
              background: 'var(--surface)',
              color: 'var(--text)',
              fontSize: 'var(--font-size-sm)',
              cursor: 'pointer',
            }}
          >
            <option value="zh">简体中文</option>
            <option value="en">English</option>
          </select>
        </div>
      </>
    );
  }

  function renderAppearance() {
    return (
      <>
        <SettingsPageHeader
          title={t(locale, 'settings.tabAppearance')}
          description={t(locale, 'settings.appearanceDesc')}
        />
        <div style={cardStyle}>
          <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.md }}>
            <Palette size={16} />
            <h3 style={{ fontSize: 'var(--font-size-md)', fontWeight: 600 }}>{t(locale, 'settings.theme')}</h3>
          </div>
          <div style={{ display: 'flex', gap: SPACING.sm }}>
            {THEMES.map(th => (
              <button
                key={th.id}
                type="button"
                aria-pressed={currentTheme === th.id}
                onClick={() => handleSelectTheme(th.id)}
                style={{
                  flex: 1, padding: `${SPACING.sm}px`, borderRadius: 'var(--radius-sm)',
                  background: currentTheme === th.id ? 'var(--primary-soft)' : 'var(--background)',
                  border: currentTheme === th.id ? '1px solid var(--primary)' : '1px solid var(--border)',
                  color: 'var(--text)', cursor: 'pointer', fontSize: 'var(--font-size-sm)', textAlign: 'center',
                }}>
                <div style={{ fontSize: 20, marginBottom: 4 }}>{th.icon}</div>
                <div>{t(locale, th.labelKey)}</div>
              </button>
            ))}
          </div>
        </div>
      </>
    );
  }

  function renderProviders() {
    if (providersError) {
      return (
        <>
          <SettingsPageHeader
            title={t(locale, 'settings.tabProviders')}
            description={t(locale, 'settings.providersDesc')}
            action={
              <button className="btn btn-primary" onClick={() => setShowAddProvider(true)}>
                + {t(locale, 'settings.addProvider')}
              </button>
            }
          />
          <InlineLoadError message={providersError} onRetry={loadProviders} locale={locale} />
        </>
      );
    }

    return (
      <>
        <SettingsPageHeader
          title={t(locale, 'settings.tabProviders')}
          description={t(locale, 'settings.providersDesc')}
          action={
            <button className="btn btn-primary" onClick={() => setShowAddProvider(true)}>
              + {t(locale, 'settings.addProvider')}
            </button>
          }
        />
        <ProviderDetail
          locale={locale} providers={providers} loading={providersLoading}
          showAddProvider={() => setShowAddProvider(true)}
          onSaveDefaults={handleSaveDefaults} onAddKey={handleAddKey} onTestKey={handleTestKey}
          onSetPrimaryKey={handleSetPrimaryKey} onDeleteKey={handleDeleteKey}
          onDeleteProvider={(id) => setDeleteProviderTarget(id)}
        />
      </>
    );
  }

  function renderPlugins() {
    return (
      <>
        <SettingsPageHeader
          title={t(locale, 'settings.tabPlugins')}
          description={t(locale, 'settings.pluginsDesc')}
          action={
            <button
              onClick={loadPlugins}
              disabled={pluginLoading}
              className="btn inline-flex items-center gap-2 text-xs"
            >
              {pluginLoading ? <Loader size={12} className="animate-spin" /> : <RefreshCw size={12} />}
              {t(locale, 'common.refresh')}
            </button>
          }
        />
        {pluginsError ? (
          <InlineLoadError message={pluginsError} onRetry={loadPlugins} locale={locale} />
        ) : pluginLoading ? (
          <div style={{ textAlign: 'center', padding: SPACING.xl, color: 'var(--text-disabled)', fontSize: 'var(--font-size-sm)' }}>
            {t(locale, 'common.loading')}
          </div>
        ) : plugins.length === 0 ? (
          <div style={cardStyle}>
            <div style={{ textAlign: 'center', padding: SPACING.xl, color: 'var(--text-disabled)', fontSize: 'var(--font-size-sm)' }}>
              <Package size={24} style={{ margin: '0 auto 8px', display: 'block', opacity: 0.5 }} />
              {t(locale, 'settings.noPlugins')}
            </div>
          </div>
        ) : (
          <div style={cardStyle}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
              {plugins.map(p => (
                <div key={p.id} style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: 'var(--radius-sm)', border: '1px solid transparent', transition: 'all 0.1s' }}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 'var(--font-size-sm)', fontWeight: 500 }}>{p.name}</div>
                    {p.version && <div style={{ fontSize: 'var(--font-size-xs)', color: 'var(--text-disabled)' }}>v{p.version}</div>}
                  </div>
                  <button onClick={() => handleTogglePlugin(p.id, p.enabled)}
                    style={{ padding: '3px 8px', borderRadius: 'var(--radius-sm)', border: '1px solid var(--border)', background: p.enabled ? 'var(--primary-soft)' : 'var(--surface)', color: 'var(--text)', cursor: 'pointer', fontSize: 'var(--font-size-xs)' }}>
                    {p.enabled ? t(locale, 'common.disable') : t(locale, 'common.enable')}
                  </button>
                  <button onClick={() => handleUninstallPlugin(p.id)}
                    style={{ padding: '3px 8px', borderRadius: 'var(--radius-sm)', border: '1px solid transparent', background: 'transparent', color: 'var(--danger)', cursor: 'pointer', fontSize: 'var(--font-size-xs)' }}>
                    <Trash size={12} />
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}
      </>
    );
  }

  const renderActiveSection = () => {
    switch (activeSection) {
      case 'general':
        return renderGeneral();
      case 'appearance':
        return renderAppearance();
      case 'providers':
        return renderProviders();
      case 'runtime':
        return <RuntimePanel locale={locale} />;
      case 'plugins':
        return renderPlugins();
    }
  };

  return (
    <div style={{ height: '100%', overflow: 'auto' }}>
      <div
        style={{
          width: 'min(100%, 760px)',
          margin: '0 auto',
          boxSizing: 'border-box',
          padding: `${SPACING.xl}px ${SPACING.lg}px ${SPACING.xxl}px`,
        }}
      >
        {renderActiveSection()}
      </div>
      {showAddProvider && <AddProviderDialog locale={locale} onClose={() => setShowAddProvider(false)} onSave={handleSaveProvider} />}
      <ConfirmDialog open={deleteProviderTarget !== null} title={t(locale, 'settings.deleteProvider')} message={t(locale, 'settings.confirmDeleteProvider')} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger
        onConfirm={() => { if (deleteProviderTarget) handleDeleteProvider(deleteProviderTarget); setDeleteProviderTarget(null); }}
        onCancel={() => setDeleteProviderTarget(null)} />
    </div>
  );
}