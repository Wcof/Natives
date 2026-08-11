'use client';

import { useState, useEffect } from 'react';
import { normalizeThemeId, applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { SPACING } from '@/lib/design-tokens';
import { Palette, Globe, Package, RefreshCw, Trash, Loader, RotateCcw, Sun, Terminal } from 'lucide-react';
import ProviderDetail from '@/components/settings/ProviderDetail';
import { ProviderSettingsWorkspace } from '@/components/settings/provider-routing/ProviderSettingsWorkspace';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import NativeHarnessPanel from '@/components/settings/NativeHarnessPanel';
import ExecutionEngineSettingsPanel from '@/components/settings/ExecutionEngineSettingsPanel';
import { UsageDashboard } from '@/components/dashboard/UsageDashboard';
import StorageOverview from '@/components/settings/StorageOverview';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';
import {
  type SettingsSection,
} from './settings-navigation';

// ── Theme skins — only light/dark as supported by the theme engine ──
const THEMES = [
  { id: 'light', labelKey: 'settings.themeJasmine', icon: <Sun size={20} /> },
  { id: 'dark', labelKey: 'settings.themeTerminal', icon: <Terminal size={20} /> },
];

/** A7: 设置 > 执行引擎 = [运行设置] [Harness 编排] 双 tab。 */
function ExecutionEngineSection({ locale }: { locale: Locale }) {
  const [tab, setTab] = useState<'runtime' | 'harness'>('runtime');
  const tabStyle = (active: boolean): React.CSSProperties => ({
    padding: '6px 14px',
    borderRadius: 6,
    border: `1px solid ${active ? 'var(--accent)' : 'var(--border)'}`,
    background: active ? 'var(--accent-soft, transparent)' : 'transparent',
    fontWeight: active ? 600 : 400,
    cursor: 'pointer',
    fontSize: 13,
  });
  return (
    <div>
      <div style={{ display: 'flex', gap: 8, marginBottom: 4 }}>
        <button type="button" style={tabStyle(tab === 'runtime')} onClick={() => setTab('runtime')}>
          {t(locale, 'executionEngine.tabRuntimeSettings')}
        </button>
        <button type="button" style={tabStyle(tab === 'harness')} onClick={() => setTab('harness')}>
          {t(locale, 'executionEngine.tabHarness')}
        </button>
      </div>
      {tab === 'runtime' ? (
        <ExecutionEngineSettingsPanel locale={locale} />
      ) : (
        <NativeHarnessPanel locale={locale} />
      )}
    </div>
  );
}

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
  activeSection = 'personal',
  locale: externalLocale,
  onNavigate: _onNavigate,
}: {
  activeSection?: SettingsSection;
  locale?: Locale;
  onNavigate?: (view: string) => void;
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
  // 默认值与主题引擎统一为 dark（theme-engine.ts normalizeThemeId 的兜底）
  const [currentTheme, setCurrentTheme] = useState('dark');
  const [currentLocale, setCurrentLocale] = useState('zh');

  // ── Plugin state ──
  const [plugins, setPlugins] = useState<Array<{ id: string; name: string; version?: string; enabled: boolean; description?: string }>>([]);
  const [pluginLoading, setPluginLoading] = useState(false);
  const [pluginsError, setPluginsError] = useState<string | null>(null);
  // Optional ccusage enrichment (default off; native scanners are authoritative).
  const [ccusageEnabled, setCcusageEnabled] = useState(false);
  const [ccusageVersion, setCcusageVersion] = useState<string | null>(null);
  const [ccusageBusy, setCcusageBusy] = useState(false);

  // Providers state
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [providersLoading, setProvidersLoading] = useState(false);
  const [providersError, setProvidersError] = useState<string | null>(null);
  const [deleteProviderTarget, setDeleteProviderTarget] = useState<string | null>(null);
  const [uninstallPluginTarget, setUninstallPluginTarget] = useState<string | null>(null);

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
        if (sl) {
          setLocaleState(sl as Locale);
          // read path 修复：此前 currentLocale 只在 appearance 分区被 loadTheme
          // 顺带加载，直接打开 general 分区时语言下拉恒显示硬编码默认「简体中文」
          setCurrentLocale(sl === 'en' ? 'en' : 'zh');
        }
      } catch { /* noop */ }
    })();
  }, []);

  // ── Theme ──
  async function loadTheme() {
    try {
      const api = window.nativesAPI;
      // 单一真实源 settings:theme（get_theme）；旧 theme_id 键回退已废除
      const saved = await api?.getTheme?.().catch(() => null);
      if (saved && typeof saved === 'string') {
        setCurrentTheme(normalizeThemeId(saved));
      }
      const loc = await api?.getLocale?.().catch(() => null);
      if (loc) setCurrentLocale(loc);
    } catch { /* ignore */ }
  }

  async function handleSelectTheme(themeId: string) {
    const normalized = normalizeThemeId(themeId);
    setCurrentTheme(normalized);
    applyTheme(normalized);
    try {
      const api = window.nativesAPI;
      if (!api?.setTheme) throw new Error('theme API unavailable');
      await api.setTheme(normalized);
    } catch (e) {
      // 持久化失败必须可见：UI 已即时换肤，静默失败会在重启后「谜之回退」
      globalToast(classifyError(e).userMessage, 'error');
    }
  }

  async function handleLocaleChange(next: string) {
    const nextLocale = next === 'en' ? 'en' : 'zh';
    setCurrentLocale(nextLocale);
    setLocaleState(nextLocale as Locale);
    try {
      const api = window.nativesAPI;
      if (!api?.setLocale) throw new Error('locale API unavailable');
      await api.setLocale(nextLocale);
    } catch (e) {
      globalToast(classifyError(e).userMessage, 'error');
    }
  }

  // ── Plugins ──
  async function loadPlugins() {
    setPluginLoading(true);
    setPluginsError(null);
    try {
      const api = window.nativesAPI;
      // Optional usage enricher status (not a web-module plugin).
      try {
        const enabled = await api?.usage?.getCcusageEnabled?.();
        setCcusageEnabled(Boolean(enabled));
        const ver = await api?.usage?.detectCcusage?.();
        setCcusageVersion(ver ?? null);
      } catch {
        setCcusageEnabled(false);
        setCcusageVersion(null);
      }

      if (!api?.module?.list) { setPlugins([]); return; }
      const list = await api.module.list() as Array<{ id: string; name?: string; version?: string; enabled: boolean; description?: string }>;
      setPlugins(list.map(p => ({ ...p, name: p.name || p.id })));
    } catch (e) {
      setPluginsError(classifyError(e).userMessage);
      setPlugins([]);
    }
    finally { setPluginLoading(false); }
  }

  async function handleToggleCcusage(next: boolean) {
    setCcusageBusy(true);
    try {
      const api = window.nativesAPI;
      if (!api?.usage?.setCcusageEnabled) throw new Error('usage API unavailable');
      if (next && !ccusageVersion) {
        // plugin_install 是 fire-and-forget（后端 spawn 线程立即返回），
        // 必须轮询 detect 等到安装真正落地，否则会出现「未安装·已启用」的假状态
        await api?.plugins?.install?.('ccusage');
        let ver: string | null = null;
        const deadline = Date.now() + 120_000;
        while (Date.now() < deadline) {
          ver = (await api?.usage?.detectCcusage?.()) ?? null;
          if (ver) break;
          await new Promise((r) => setTimeout(r, 2000));
        }
        setCcusageVersion(ver);
        if (!ver) {
          throw new Error(t(locale, 'settings.ccusageInstallTimeout'));
        }
      }
      const enabled = await api.usage.setCcusageEnabled(next);
      setCcusageEnabled(Boolean(enabled));
      globalToast(
        next ? t(locale, 'settings.ccusageEnabledToast') : t(locale, 'settings.ccusageDisabledToast'),
        'success',
      );
    } catch (e) {
      globalToast(classifyError(e).userMessage, 'error');
    } finally {
      setCcusageBusy(false);
      await loadPlugins();
    }
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
      globalToast(t(locale, 'settings.pluginUninstalled'), 'success');
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

  async function handleDeleteProvider(id: string) {
    try {
      const a = window.nativesAPI;
      if (!a?.provider?.delete) throw new Error('provider API unavailable');
      await a.provider.delete(id);
      globalToast(t(locale, 'settings.providerDeleted'), 'success');
      await loadProviders();
    } catch (e) {
      // 原实现无 catch：删除失败 = unhandled rejection + 列表残留无解释
      globalToast(classifyError(e).userMessage, 'error');
    }
  }
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
            <div><h3>{t(locale, 'settings.language')}</h3><p>{t(locale, 'settingsPage.languageDesc')}</p></div>
          </div>
          <select
            className="settings-select"
            value={currentLocale}
            onChange={(event) =>
              void handleLocaleChange(event.target.value)
            }
          >
            <option value="zh">{t(locale, 'settingsPage.langChinese')}</option>
            <option value="en">{t(locale, 'settingsPage.langEnglish')}</option>
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
            <div><h4>{t(locale, 'settings.theme')}</h4><p>{t(locale, 'settingsPage.themeDesc')}</p></div>
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
        <ProviderSettingsWorkspace locale={locale} providers={providers} onProviderCreated={loadProviders} management={
          <ProviderDetail
            locale={locale} providers={providers} loading={providersLoading}
            showAddProvider={() => setShowAddProvider(true)}
            onSaveDefaults={handleSaveDefaults} onAddKey={handleAddKey} onTestKey={handleTestKey}
            onSetPrimaryKey={handleSetPrimaryKey} onDeleteKey={handleDeleteKey}
            onDeleteProvider={(id) => setDeleteProviderTarget(id)}
          />
        } />
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

        {/* Optional ccusage enricher — not required for dashboard accuracy */}
        <div className="settings-section-card" style={{ marginBottom: 12 }}>
          <div className="settings-plugin-row">
            <span className="settings-preference-icon"><Package size={17} /></span>
            <div className="settings-plugin-copy">
              <strong>ccusage</strong>
              <span>
                {t(locale, 'settingsPage.ccusageDesc')}
              </span>
              <small>
                {ccusageVersion
                  ? `v${ccusageVersion}`
                  : t(locale, 'settingsPage.notInstalled')}
                {' · '}
                {ccusageEnabled
                  ? t(locale, 'settingsPage.enabled')
                  : t(locale, 'settingsPage.disabledDefault')}
              </small>
            </div>
            <button
              onClick={() => handleToggleCcusage(!ccusageEnabled)}
              disabled={ccusageBusy || pluginLoading}
              className={`settings-status-button${ccusageEnabled ? ' enabled' : ''}`}
            >
              {ccusageBusy
                ? t(locale, 'settingsPage.working')
                : ccusageEnabled
                  ? t(locale, 'common.disable')
                  : t(locale, 'common.enable')}
            </button>
          </div>
        </div>

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
                    <span>{p.description || t(locale, 'settingsPage.pluginNoDescription')}</span>
                    {p.version && <small>v{p.version}</small>}
                  </div>
                  <button onClick={() => handleTogglePlugin(p.id, p.enabled)}
                    className={`settings-status-button${p.enabled ? ' enabled' : ''}`}>
                    {p.enabled ? t(locale, 'common.disable') : t(locale, 'common.enable')}
                  </button>
                  {/* 破坏性操作必须确认（与 provider 删除对称） */}
                  <button onClick={() => setUninstallPluginTarget(p.id)} className="settings-icon-button danger"
                    title={t(locale, 'common.uninstall')} aria-label={t(locale, 'common.uninstall')}>
                    <Trash size={12} />
                  </button>
                </div>
              ))}
          </div>
        )}
      </>
    );
  }

  const renderActiveSection = () => {
    switch (activeSection) {
      case 'personal':
        // 问题7：设置页直接复用个人主页 UsageDashboard（同一筛选/图表/缓存/
        // 同步状态机），仅通过既有 children 接缝追加真实 StorageOverview。
        return (
          <UsageDashboard>
            <StorageOverview />
          </UsageDashboard>
        );
      case 'general':
        return renderGeneral();
      case 'appearance':
        return renderAppearance();
      case 'providers':
        return renderProviders();
      case 'runtime':
        return <ExecutionEngineSection locale={locale} />;
      case 'plugins':
        return renderPlugins();
    }
  };

  return (
    <div style={{ height: '100%', overflow: 'auto' }}>
      <div
        style={{
          width: activeSection === 'runtime' || activeSection === 'personal' ? 'min(100%, 1380px)' : 'min(100%, 920px)',
          margin: '0 auto',
          boxSizing: 'border-box',
          padding: `${SPACING.xl}px ${SPACING.lg}px ${SPACING.xxl}px`,
        }}
      >
        {renderActiveSection()}
      </div>
      {showAddProvider && <AddProviderDialog locale={locale} onClose={() => setShowAddProvider(false)} onSave={handleSaveProvider} />}
      <ConfirmDialog open={deleteProviderTarget !== null} title={t(locale, 'settings.deleteProvider')} message={t(locale, 'settings.confirmDeleteProvider')} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger
        onConfirm={() => { if (deleteProviderTarget) void handleDeleteProvider(deleteProviderTarget); setDeleteProviderTarget(null); }}
        onCancel={() => setDeleteProviderTarget(null)} />
      <ConfirmDialog open={uninstallPluginTarget !== null} title={t(locale, 'common.uninstall')} message={t(locale, 'settings.confirmUninstallPlugin')} confirmLabel={t(locale, 'common.uninstall')} cancelLabel={t(locale, 'common.cancel')} danger
        onConfirm={() => { if (uninstallPluginTarget) void handleUninstallPlugin(uninstallPluginTarget); setUninstallPluginTarget(null); }}
        onCancel={() => setUninstallPluginTarget(null)} />
    </div>
  );
}
