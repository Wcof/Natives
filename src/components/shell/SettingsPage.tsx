'use client';

import { useState, useEffect, useCallback } from 'react';
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
import PersonalOverview from '@/components/settings/PersonalOverview';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';
import {
  type SettingsSection,
} from './settings-navigation';
import type {
  CreativeAppDockerStatus,
  CreativeAppGithubTokenStatus,
  LocalCreativeAiSettings,
} from '@/lib/tauri-adapter';

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

function CreativeRuntimeSettings({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const [docker, setDocker] = useState<CreativeAppDockerStatus | null>(null);
  const [token, setToken] = useState<CreativeAppGithubTokenStatus | null>(null);
  const [tokenInput, setTokenInput] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    try {
      const api = window.nativesAPI?.creativeApp;
      const [d, tstat] = await Promise.all([
        api?.dockerStatus?.() ?? null,
        api?.githubTokenStatus?.() ?? null,
      ]);
      setDocker(d);
      setToken(tstat);
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    } finally {
      setBusy(false);
    }
  }, [toast]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="settings-section-card" style={{ marginTop: 16 }}>
      <div className="settings-section-heading">
        <div>
          <h4>{t(locale, 'settings.creativeRuntime')}</h4>
          <p>{t(locale, 'settings.creativeRuntimeDesc')}</p>
        </div>
        <button type="button" className="btn btn-secondary" onClick={() => void refresh()} disabled={busy}>
          <RefreshCw size={12} /> {t(locale, 'settings.dockerRefresh')}
        </button>
      </div>
      <div style={{ display: 'grid', gap: 10, fontSize: 13 }}>
        <div>
          <strong>{t(locale, 'settings.dockerEngine')}</strong>:{' '}
          {/* null=尚未检测完成，不得与「不可用」混同显示（假诊断） */}
          {docker === null
            ? t(locale, 'common.loading')
            : docker.available
              ? `${t(locale, 'settings.dockerAvailable')}${docker.version ? ` (${docker.version})` : ''}`
              : t(locale, 'settings.dockerUnavailable')}
          {docker?.error ? (
            <div style={{ color: 'var(--danger)', fontSize: 12 }}>{docker.error}</div>
          ) : null}
        </div>
        <div>
          <strong>{t(locale, 'settings.dockerCompose')}</strong>:{' '}
          {docker === null
            ? t(locale, 'common.loading')
            : docker.composeAvailable
              ? `${t(locale, 'settings.dockerAvailable')}${docker.composeVersion ? ` (${docker.composeVersion})` : ''}`
              : t(locale, 'settings.dockerUnavailable')}
        </div>
        <div>
          <strong>{t(locale, 'settings.githubToken')}</strong>:{' '}
          {token?.configured
            ? token.masked
            : t(locale, 'settings.githubTokenNotSet')}
          <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 4 }}>
            {t(locale, 'settings.githubTokenHint')}
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
            <input
              type="password"
              value={tokenInput}
              onChange={(e) => setTokenInput(e.target.value)}
              placeholder="ghp_…"
              style={{
                flex: 1,
                padding: '6px 8px',
                borderRadius: 6,
                border: '1px solid var(--border)',
                background: 'var(--bg-2)',
                color: 'var(--text)',
                fontSize: 12,
              }}
            />
            <button
              type="button"
              className="btn btn-primary"
              disabled={!tokenInput.trim() || busy}
              onClick={async () => {
                try {
                  const api = window.nativesAPI?.creativeApp;
                  // API 不在时可选链返回 undefined 也会走成功 toast（假成功），必须显式拒绝
                  if (!api?.githubTokenSet) throw new Error('creativeApp API unavailable');
                  const st = await api.githubTokenSet(tokenInput.trim());
                  setToken(st ?? null);
                  setTokenInput('');
                  toast(t(locale, 'settings.githubTokenSaved'), 'success');
                } catch (e) {
                  toast(classifyError(e).userMessage, 'error');
                }
              }}
            >
              {t(locale, 'settings.githubTokenSet')}
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              disabled={!token?.configured || busy}
              onClick={async () => {
                try {
                  const st = await window.nativesAPI?.creativeApp?.githubTokenClear?.();
                  setToken(st ?? null);
                  toast(t(locale, 'settings.githubTokenCleared'), 'success');
                } catch (e) {
                  toast(classifyError(e).userMessage, 'error');
                }
              }}
            >
              {t(locale, 'settings.githubTokenClear')}
            </button>
          </div>
        </div>
      </div>

      <LocalCreativeAiSettingsPanel locale={locale} />
    </div>
  );
}

function LocalCreativeAiSettingsPanel({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const [settings, setSettings] = useState<LocalCreativeAiSettings | null>(null);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setBusy(true);
    try {
      const st =
        (await window.nativesAPI?.creativeApp?.getLocalAiSettings?.()) ??
        ({
          enabled: false,
          mode: 'only_when_uncertain',
          userConsented: false,
          timeoutMs: 45000,
        } as LocalCreativeAiSettings);
      setSettings(st);
      const list = (await window.nativesAPI?.provider?.list?.()) as ProviderSummary[] | undefined;
      setProviders(Array.isArray(list) ? list : []);
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    } finally {
      setBusy(false);
    }
  }, [toast]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!settings) return null;

  const save = async (next: LocalCreativeAiSettings) => {
    try {
      const api = window.nativesAPI?.creativeApp;
      if (!api?.saveLocalAiSettings) throw new Error('creativeApp API unavailable');
      const saved = await api.saveLocalAiSettings(next);
      setSettings(saved ?? next);
      toast(t(locale, 'settings.localAiSaved'), 'success');
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    }
  };

  return (
    <div className="settings-section-card" style={{ marginTop: 16 }}>
      <div className="settings-section-heading">
        <div>
          <h4>{t(locale, 'settings.localAiTitle')}</h4>
          <p>{t(locale, 'settings.localAiDesc')}</p>
        </div>
        <button type="button" className="btn btn-secondary" onClick={() => void refresh()} disabled={busy}>
          <RefreshCw size={12} /> {t(locale, 'common.refresh')}
        </button>
      </div>
      <div style={{ display: 'grid', gap: 10, fontSize: 13 }}>
        <label style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <input
            type="checkbox"
            checked={settings.enabled}
            onChange={(e) => void save({ ...settings, enabled: e.target.checked })}
          />
          {t(locale, 'settings.localAiEnabled')}
        </label>
        <label style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <input
            type="checkbox"
            checked={settings.userConsented}
            onChange={(e) => void save({ ...settings, userConsented: e.target.checked })}
          />
          {t(locale, 'settings.localAiConsent')}
        </label>
        <div>
          <div style={{ marginBottom: 4 }}>{t(locale, 'settings.localAiMode')}</div>
          <select
            value={settings.mode}
            onChange={(e) => void save({ ...settings, mode: e.target.value })}
            style={{
              width: '100%',
              padding: '6px 8px',
              borderRadius: 6,
              border: '1px solid var(--border)',
              background: 'var(--bg-2)',
              color: 'var(--text)',
            }}
          >
            <option value="only_when_uncertain">{t(locale, 'settings.localAiModeUncertain')}</option>
            <option value="always">{t(locale, 'settings.localAiModeAlways')}</option>
          </select>
        </div>
        <div>
          <div style={{ marginBottom: 4 }}>{t(locale, 'settings.localAiProvider')}</div>
          <select
            value={settings.providerId || ''}
            onChange={(e) =>
              void save({
                ...settings,
                providerId: e.target.value || null,
              })
            }
            style={{
              width: '100%',
              padding: '6px 8px',
              borderRadius: 6,
              border: '1px solid var(--border)',
              background: 'var(--bg-2)',
              color: 'var(--text)',
            }}
          >
            <option value="">{t(locale, 'settings.localAiProviderNone')}</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.displayName || p.id}
              </option>
            ))}
          </select>
        </div>
        <div>
          <div style={{ marginBottom: 4 }}>{t(locale, 'settings.localAiModel')}</div>
          <input
            value={settings.model || ''}
            onChange={(e) => setSettings({ ...settings, model: e.target.value })}
            onBlur={() => void save(settings)}
            placeholder="gpt-4.1-mini / claude-…"
            style={{
              width: '100%',
              padding: '6px 8px',
              borderRadius: 6,
              border: '1px solid var(--border)',
              background: 'var(--bg-2)',
              color: 'var(--text)',
              fontSize: 12,
            }}
          />
        </div>
      </div>
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
  onNavigate,
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
        return <PersonalOverview locale={locale} onNavigate={onNavigate} />;
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
