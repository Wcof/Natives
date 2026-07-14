'use client';

import { useState, useEffect, useCallback } from 'react';
import { normalizeThemeId, applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { Trash2, Monitor, Palette, Globe, Puzzle, Package, RefreshCw, Trash, Download, Loader } from 'lucide-react';
import RuntimePanel from '@/components/assistant/RuntimePanel';
import ProviderDetail from '@/components/settings/ProviderDetail';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';

// ── Theme skins ──
const THEMES = [
  { id: 'terminal', labelKey: 'settings.themeTerminal', icon: '🖥' },
  { id: 'warm', labelKey: 'settings.themeWarm', icon: '🌅' },
  { id: 'editorial', labelKey: 'settings.themeEditorial', icon: '📝' },
];

export default function SettingsPage() {
  const { toast: globalToast } = useToast();
  const [locale, setLocaleState] = useState<Locale>('zh');
  const [activeTab, setActiveTab] = useState<'theme' | 'env' | 'plugins' | 'executor' | 'providers'>('theme');
  const TABS = [
    { id: 'theme' as const, label: t(locale, 'settings.tabTheme') },
    { id: 'env' as const, label: t(locale, 'settings.tabEnv') },
    { id: 'plugins' as const, label: t(locale, 'settings.tabPlugins') },
    { id: 'executor' as const, label: t(locale, 'settings.tabExecutor') },
    { id: 'providers' as const, label: t(locale, 'settings.tabProviders') },
  ];

  // ── Theme state ──
  const [currentTheme, setCurrentTheme] = useState('terminal');
  const [currentLocale, setCurrentLocale] = useState('zh');

  // ── Env state ──
  const [envProfiles, setEnvProfiles] = useState<Array<{ id: string; name: string }>>([]);
  const [envVars, setEnvVars] = useState<Array<{ key: string; value: string }>>([]);
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [newProfileName, setNewProfileName] = useState('');

  // ── Plugin state ──
  const [plugins, setPlugins] = useState<Array<{ id: string; name: string; version?: string; enabled: boolean; description?: string }>>([]);
  const [pluginLoading, setPluginLoading] = useState(false);
  const [installingPlugin, setInstallingPlugin] = useState<string | null>(null);

  // Providers state
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [providersLoading, setProvidersLoading] = useState(false);
  const [deleteProviderTarget, setDeleteProviderTarget] = useState<string | null>(null);

  useEffect(() => {
    if (activeTab === 'providers') loadProviders();
    if (activeTab === 'theme') loadTheme();
    if (activeTab === 'env') loadEnv();
    if (activeTab === 'plugins') loadPlugins();
  }, [activeTab]);

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

  async function handleToggleLocale() {
    const next = currentLocale === 'zh' ? 'en' : 'zh';
    setCurrentLocale(next);
    setLocaleState(next as Locale);
    try {
      await window.nativesAPI?.setLocale?.(next);
      if ((window.nativesAPI as any)?.locale?.setLocale) await (window.nativesAPI as any).locale.setLocale(next);
    } catch { /* ignore */ }
  }

  // ── Env ──
  async function loadEnv() {
    try {
      const api = window.nativesAPI;
      if (!api?.env?.listProfiles) return;
      const profiles = await api.env.listProfiles() as unknown as Array<{ id: string; name: string }>;
      setEnvProfiles(profiles);
      const defaultProfile = await api.env.getDefaultProfile() as unknown as { id: string; name: string } | null;
      if (defaultProfile) {
        setActiveProfileId(defaultProfile.id);
        const vars = await api.env.getVariables(defaultProfile.id) as Array<{ key: string; value: string }>;
        setEnvVars(vars);
      }
    } catch { /* ignore */ }
  }

  async function handleCreateProfile() {
    if (!newProfileName.trim()) return;
    try {
      const api = window.nativesAPI;
      if (!api?.env?.createProfile) return;
      await api.env.createProfile(newProfileName.trim());
      setNewProfileName('');
      await loadEnv();
    } catch (e) { globalToast(classifyError(e).userMessage, 'error'); }
  }

  // ── Plugins ──
  async function loadPlugins() {
    setPluginLoading(true);
    try {
      const api = window.nativesAPI;
      if (!api?.module?.list) { setPlugins([]); return; }
      const list = await api.module.list() as Array<{ id: string; name?: string; version?: string; enabled: boolean; description?: string }>;
      setPlugins(list.map(p => ({ ...p, name: p.name || p.id })));
    } catch { setPlugins([]); }
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
    try {
      const api = window.nativesAPI;
      if (api?.provider?.list) setProviders(Array.isArray(await api.provider.list()) ? await api.provider.list() as unknown as ProviderSummary[] : []);
    } catch (e) { globalToast(classifyError(e).userMessage, 'error'); }
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

  const btnStyle: React.CSSProperties = { flex: 1, textAlign: 'center', padding: '7px 4px', borderRadius: 'calc(var(--radius-sm) - 2px)', cursor: 'pointer', border: '1px solid transparent', fontSize: FONT_SIZE.md, transition: 'all 0.12s' };
  const cardStyle: React.CSSProperties = { background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: SPACING.md };
  const inpStyle: React.CSSProperties = { width: '100%', boxSizing: 'border-box', padding: '5px 8px', borderRadius: 'calc(var(--radius-sm) - 2px)', border: '1px solid var(--border)', background: 'var(--background)', color: 'var(--text)', fontSize: FONT_SIZE.xs, outline: 'none' };

  return (
    <div style={{height:'100%',overflow:'auto'}}>
      <div style={{padding:`${SPACING.lg}px 20px`,display:'flex',flexDirection:'column',gap:28}}>
        <div style={{display:'flex',gap:4,background:'var(--surface-hover)',border:'1px solid var(--border)',borderRadius:'var(--radius-sm)',padding:3}}>
          {TABS.map(tab => {
            const isActive = activeTab === tab.id;
            return <button key={tab.id} onClick={() => setActiveTab(tab.id)}
              style={{...btnStyle, background: isActive ? 'var(--primary-soft)' : 'transparent', color: isActive ? 'var(--primary)' : 'var(--text-secondary)', border: isActive ? '1px solid var(--primary)' : '1px solid transparent', fontWeight: isActive ? 600 : 400 }}>
              {tab.label}
            </button>;
          })}
        </div>

        {/* ─── Theme Tab ─── */}
        <div style={{ display: activeTab === 'theme' ? 'flex' : 'none', flexDirection: 'column', gap: SPACING.lg }}>
          <div style={cardStyle}>
            <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.md }}>
              <Palette size={16} />
              <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600 }}>{t(locale, 'settings.theme')}</h3>
            </div>
            <div style={{ display: 'flex', gap: SPACING.sm }}>
              {THEMES.map(th => (
                <button key={th.id} onClick={() => handleSelectTheme(th.id)}
                  style={{
                    flex: 1, padding: `${SPACING.sm}px`, borderRadius: 'var(--radius-sm)',
                    background: currentTheme === th.id ? 'var(--primary-soft)' : 'var(--background)',
                    border: currentTheme === th.id ? '1px solid var(--primary)' : '1px solid var(--border)',
                    color: 'var(--text)', cursor: 'pointer', fontSize: FONT_SIZE.sm, textAlign: 'center',
                  }}>
                  <div style={{ fontSize: 20, marginBottom: 4 }}>{th.icon}</div>
                  <div>{t(locale, th.labelKey)}</div>
                </button>
              ))}
            </div>
          </div>

          <div style={cardStyle}>
            <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.md }}>
              <Globe size={16} />
              <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600 }}>{t(locale, 'settings.language')}</h3>
            </div>
            <button onClick={handleToggleLocale}
              style={{ padding: `${SPACING.xs}px ${SPACING.md}px`, borderRadius: 'var(--radius-sm)', border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', cursor: 'pointer', fontSize: FONT_SIZE.sm }}>
              {currentLocale === 'zh' ? '切换为 English' : 'Switch to 中文'}
            </button>
          </div>
        </div>

        {/* ─── Env Tab ─── */}
        <div style={{ display: activeTab === 'env' ? 'flex' : 'none', flexDirection: 'column', gap: SPACING.lg }}>
          <div style={cardStyle}>
            <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.md }}>
              <Monitor size={16} />
              <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600 }}>{t(locale, 'settings.environmentProfiles')}</h3>
            </div>
            <div style={{ display: 'flex', gap: SPACING.sm, marginBottom: SPACING.md }}>
              <input value={newProfileName} onChange={e => setNewProfileName(e.target.value)} placeholder={t(locale, 'settings.newProfileName')} style={inpStyle} />
              <button onClick={handleCreateProfile} disabled={!newProfileName.trim()}
                style={{ padding: '5px 12px', borderRadius: 'var(--radius-sm)', border: '1px solid var(--border)', background: 'var(--primary)', color: '#fff', cursor: 'pointer', fontSize: FONT_SIZE.xs, whiteSpace: 'nowrap' }}>
                {t(locale, 'common.create')}
              </button>
            </div>
            {envProfiles.length === 0 ? (
              <div style={{ color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs }}>{t(locale, 'settings.noProfiles')}</div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                {envProfiles.map(p => (
                  <div key={p.id} style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: '4px 8px', borderRadius: 'var(--radius-sm)', background: p.id === activeProfileId ? 'var(--primary-soft)' : 'transparent' }}>
                    <span style={{ flex: 1, fontSize: FONT_SIZE.sm }}>{p.name}</span>
                    {p.id === activeProfileId && <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--primary)' }}>{t(locale, 'settings.active')}</span>}
                  </div>
                ))}
              </div>
            )}
          </div>

          {activeProfileId && (
            <div style={cardStyle}>
              <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600, marginBottom: SPACING.sm }}>{t(locale, 'settings.environmentVariables')}</h3>
              {envVars.length === 0 ? (
                <div style={{ color: 'var(--text-disabled)', fontSize: FONT_SIZE.xs }}>{t(locale, 'settings.noVariables')}</div>
              ) : (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  {envVars.map((v, i) => (
                    <div key={i} style={{ display: 'flex', gap: SPACING.sm, padding: '3px 0', fontSize: FONT_SIZE.xs }}>
                      <code style={{ color: 'var(--primary)', minWidth: 120 }}>{v.key}</code>
                      <span style={{ color: 'var(--text-secondary)', fontFamily: 'monospace' }}>••••••••</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* ─── Plugins Tab ─── */}
        <div style={{ display: activeTab === 'plugins' ? 'flex' : 'none', flexDirection: 'column', gap: SPACING.lg }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, marginBottom: SPACING.sm }}>
            <Puzzle size={16} />
            <h3 style={{ fontSize: FONT_SIZE.md, fontWeight: 600, flex: 1 }}>{t(locale, 'settings.tabPlugins')}</h3>
            <button onClick={loadPlugins} disabled={pluginLoading}
              style={{ display: 'inline-flex', alignItems: 'center', gap: 4, padding: '5px 10px', borderRadius: 'var(--radius-sm)', border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', cursor: 'pointer', fontSize: FONT_SIZE.xs }}>
              {pluginLoading ? <Loader size={12} className="animate-spin" /> : <RefreshCw size={12} />}
              {t(locale, 'common.refresh')}
            </button>
          </div>

          <div style={cardStyle}>
            {pluginLoading ? (
              <div style={{ textAlign: 'center', padding: SPACING.xl, color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm }}>{t(locale, 'common.loading')}</div>
            ) : plugins.length === 0 ? (
              <div style={{ textAlign: 'center', padding: SPACING.xl, color: 'var(--text-disabled)', fontSize: FONT_SIZE.sm }}>
                <Package size={24} style={{ margin: '0 auto 8px', display: 'block', opacity: 0.5 }} />
                {t(locale, 'settings.noPlugins')}
              </div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
                {plugins.map(p => (
                  <div key={p.id} style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm, padding: `${SPACING.xs}px ${SPACING.sm}px`, borderRadius: 'var(--radius-sm)', border: '1px solid transparent', transition: 'all 0.1s' }}>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 500 }}>{p.name}</div>
                      {p.version && <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)' }}>v{p.version}</div>}
                    </div>
                    <button onClick={() => handleTogglePlugin(p.id, p.enabled)}
                      style={{ padding: '3px 8px', borderRadius: 'var(--radius-sm)', border: '1px solid var(--border)', background: p.enabled ? 'var(--primary-soft)' : 'var(--surface)', color: 'var(--text)', cursor: 'pointer', fontSize: FONT_SIZE.xs }}>
                      {p.enabled ? t(locale, 'common.disable') : t(locale, 'common.enable')}
                    </button>
                    <button onClick={() => handleUninstallPlugin(p.id)}
                      style={{ padding: '3px 8px', borderRadius: 'var(--radius-sm)', border: '1px solid transparent', background: 'transparent', color: 'var(--danger)', cursor: 'pointer', fontSize: FONT_SIZE.xs }}>
                      <Trash size={12} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>

        {/* ─── Executor Tab ─── */}
        <div style={{ display: activeTab === 'executor' ? 'block' : 'none' }}><RuntimePanel locale={locale} /></div>

        {/* ─── Providers Tab ─── */}
        <div style={{ display: activeTab === 'providers' ? 'block' : 'none' }}>
          <ProviderDetail
            locale={locale} providers={providers} loading={providersLoading}
            showAddProvider={() => setShowAddProvider(true)}
            onSaveDefaults={handleSaveDefaults} onAddKey={handleAddKey} onTestKey={handleTestKey}
            onSetPrimaryKey={handleSetPrimaryKey} onDeleteKey={handleDeleteKey}
            onDeleteProvider={(id) => setDeleteProviderTarget(id)}
          />
        </div>
      </div>
      {showAddProvider && <AddProviderDialog locale={locale} onClose={() => setShowAddProvider(false)} onSave={handleSaveProvider} />}
      <ConfirmDialog open={deleteProviderTarget !== null} title={t(locale, 'settings.deleteProvider')} message={t(locale, 'settings.confirmDeleteProvider')} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger
        onConfirm={() => { if (deleteProviderTarget) handleDeleteProvider(deleteProviderTarget); setDeleteProviderTarget(null); }}
        onCancel={() => setDeleteProviderTarget(null)} />
    </div>
  );
}
