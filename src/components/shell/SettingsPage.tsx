'use client';

import { useState, useEffect } from 'react';
import { normalizeThemeId, applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { SPACING } from '@/lib/design-tokens';
import { Palette, Globe, Package, RefreshCw, Trash, Loader, RotateCcw, Sun, Terminal } from 'lucide-react';
import RuntimePanel from '@/components/assistant/RuntimePanel';
import ProviderDetail from '@/components/settings/ProviderDetail';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import EngineCapabilitiesPanel from '@/components/settings/EngineCapabilitiesPanel';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';
import {
  type SettingsSection,
} from './settings-navigation';

// ── Theme skins — only light/dark as supported by the theme engine ──
const THEMES = [
  { id: 'light', labelKey: 'settings.themeJasmine', icon: <Sun size={20} /> },
  { id: 'dark', labelKey: 'settings.themeTerminal', icon: <Terminal size={20} /> },
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
    <header className="settings-page-header">
      <div>
        <h2>
          {title}
        </h2>
        <p>
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
      if (api?.getTheme) {
        const saved = await api.getTheme().catch(() => null);
        if (saved && typeof saved === 'string') {
          const normalized = normalizeThemeId(saved);
          setCurrentTheme(normalized);
        }
      } else {
        const saved = await api?.db?.get('theme_id').catch(() => null);
        if (saved && typeof saved === 'string') {
          const normalized = normalizeThemeId(saved);
          setCurrentTheme(normalized);
        }
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
      const api = window.nativesAPI;
      if (api?.setTheme) {
        await api.setTheme(normalized);
      }
      await api?.db?.set('theme_id', normalized);
    } catch { /* persist best-effort */ }
  }

  async function handleLocaleChange(next: string) {
    const nextLocale = next === 'en' ? 'en' : 'zh';
    setCurrentLocale(nextLocale);
    setLocaleState(nextLocale as Locale);
    try {
      await window.nativesAPI?.setLocale?.(nextLocale);
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

  async function handleSaveProvider(data: { providerType: string; apiProtocol: string; name: string; websiteUrl: string; baseUrl: string; defaultModel: string; keys: { label: string; apiKey: string }[] }) {
    const api = window.nativesAPI;
    if (api?.provider?.create) {
      const initialKey = data.keys[0] ? { label: data.keys[0].label, apiKey: data.keys[0].apiKey } : { label: 'default', apiKey: '' };
      await api.provider.create({ providerType: data.providerType, apiProtocol: data.apiProtocol, displayName: data.name, websiteUrl: data.websiteUrl, baseUrl: data.baseUrl, defaultModel: data.defaultModel, initialKey });
    } else throw new Error('Provider API not available');
    globalToast(t(locale, 'settings.providerAdded'), 'success');
    await loadProviders();
  }

  async function handleDeleteProvider(id: string) { const a = window.nativesAPI; if (!a?.provider?.delete) return; await a.provider.delete(id); globalToast(t(locale, 'settings.providerDeleted'), 'success'); await loadProviders(); }
  async function handleSaveDefaults(pid: string, m: string | null) { const a = window.nativesAPI; if (a?.provider?.updateDefaults) { await a.provider.updateDefaults({ providerId: pid, defaultModel: m ?? '' }); await loadProviders(); } else throw new Error('updateDefaults'); }
  async function handleAddKey(pid: string, l: string, k: string) { const a = window.nativesAPI; if (a?.provider?.addKey) await a.provider.addKey({ providerId: pid, label: l, apiKey: k }); else throw new Error('addKey'); await loadProviders(); }
  async function handleTestKey(pid: string, kid: string, model?: string): Promise<TestKeyResult> { const a = window.nativesAPI; if (a?.provider?.testKey) { const r = await a.provider.testKey({ providerId: pid, keyId: kid, model }); await loadProviders(); return r as unknown as TestKeyResult; } throw new Error('testKey'); }
  async function handleSetPrimaryKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.setPrimaryKey) { await a.provider.setPrimaryKey({ providerId: pid, keyId: kid }); await loadProviders(); } else throw new Error('setPrimaryKey'); }
  async function handleDeleteKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.deleteKey) await a.provider.deleteKey({ providerId: pid, keyId: kid }); else throw new Error('deleteKey'); await loadProviders(); }

  // ── Render sections ──

  function renderGeneral() {
    return (
      <>
        <SettingsPageHeader
          title={t(locale, 'settings.tabGeneral')}
          description={t(locale, 'settings.generalDesc')}
        />
        <div className="settings-preference-card">
          <div className="settings-preference-copy">
            <span className="settings-preference-icon"><Globe size={18} /></span>
            <div><h3>{t(locale, 'settings.language')}</h3><p>{locale === 'zh' ? '选择应用界面使用的语言。' : 'Choose the language used by the interface.'}</p></div>
          </div>
          <select
            className="settings-select"
            value={currentLocale}
            onChange={(event) =>
              void handleLocaleChange(event.target.value)
            }
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
        <div className="settings-section-card">
          <div className="settings-section-heading settings-section-heading-with-icon">
            <span className="settings-preference-icon"><Palette size={18} /></span>
            <div><h4>{t(locale, 'settings.theme')}</h4><p>{locale === 'zh' ? '主题会立即应用，并自动保存。' : 'Theme changes apply immediately and save automatically.'}</p></div>
          </div>
          <div className="settings-theme-grid">
            {THEMES.map(th => {
              const isSelected = currentTheme === th.id;
              return (
                <button
                  key={th.id}
                  type="button"
                  aria-pressed={isSelected}
                  onClick={() => handleSelectTheme(th.id)}
                  className={`settings-theme-option${isSelected ? ' selected' : ''}`}
                >
                  <div className="settings-theme-preview">
                    {th.icon}
                  </div>
                  <div><strong>{t(locale, th.labelKey)}</strong><span>{t(locale, th.id === 'dark' ? 'settings.themeDescTerminal' : 'settings.themeDescJasmine')}</span></div>
                </button>
              );
            })}
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
          <div className="settings-section-card">
            <div className="settings-plugin-empty">
              <Package size={24} style={{ margin: '0 auto 8px', display: 'block', opacity: 0.5 }} />
              {t(locale, 'settings.noPlugins')}
            </div>
          </div>
        ) : (
          <div className="settings-section-card settings-plugin-list">
              {plugins.map(p => (
                <div key={p.id} className="settings-plugin-row">
                  <span className="settings-preference-icon"><Package size={17} /></span>
                  <div className="settings-plugin-copy">
                    <strong>{p.name}</strong>
                    <span>{p.description || (locale === 'zh' ? '暂无插件说明' : 'No description')}</span>
                    {p.version && <small>v{p.version}</small>}
                  </div>
                  <button onClick={() => handleTogglePlugin(p.id, p.enabled)}
                    className={`settings-status-button${p.enabled ? ' enabled' : ''}`}>
                    {p.enabled ? t(locale, 'common.disable') : t(locale, 'common.enable')}
                  </button>
                  <button onClick={() => handleUninstallPlugin(p.id)} className="settings-icon-button danger">
                    <Trash size={12} />
                  </button>
                </div>
              ))}
          </div>
        )}
      </>
    );
  }

  function renderEngineCaps() {
    return (
      <>
        <SettingsPageHeader
          title={t(locale, 'settings.tabEngineCaps')}
          description={t(locale, 'settings.engineCapsDesc')}
        />
        <EngineCapabilitiesPanel locale={locale} />
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
      case 'engine':
        return renderEngineCaps();
      case 'plugins':
        return renderPlugins();
    }
  };

  return (
    <div style={{ height: '100%', overflow: 'auto' }}>
      <div
        style={{
          width: 'min(100%, 920px)',
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
