'use client';

import { useState, useEffect, useCallback } from 'react';
import { applyTheme, normalizeThemeId } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { X, Check, Star, Edit2, Download, RefreshCw, Trash2, ExternalLink, AlertCircle, Terminal } from 'lucide-react';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import RuntimePanel from '@/components/assistant/RuntimePanel';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import type { UserProvider } from '@/types/provider';

interface EnvProfile {
  id: number;
  name: string;
  is_default: number;
  created_at: string;
}

interface EnvVariable {
  key: string;
  value: string;
}

export default function SettingsPage() {
  const { toast: globalToast } = useToast();
  const [theme, setThemeState] = useState('dark');
  const [locale, setLocaleState] = useState<Locale>('zh');
  const [sidebarWidth, setSidebarWidth] = useState(248);
  const [panelWidth, setPanelWidth] = useState(320);
  const [terminalHeight, setTerminalHeight] = useState(280);

  // Environment profiles state
  const [profiles, setProfiles] = useState<EnvProfile[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<string | null>(null);
  const [variables, setVariables] = useState<EnvVariable[]>([]);
  const [newProfileName, setNewProfileName] = useState('');
  const [showNewProfile, setShowNewProfile] = useState(false);
  const [newVarKey, setNewVarKey] = useState('');
  const [newVarValue, setNewVarValue] = useState('');
  const [showNewVar, setShowNewVar] = useState(false);
  const [editingVar, setEditingVar] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [toast, setToast] = useState<string | null>(null);
  // 执行引擎设置（PRD 3.4）：工具开关 + 自愈上限，持久化到 DB
  const [enabledTools, setEnabledTools] = useState<Record<string, boolean>>({
    read_file: true, list_dir: true, write_file: true,
    write_module: true, run_terminal: false, lint_module: true,
  });
  const [maxSelfHeal, setMaxSelfHeal] = useState(3);
  const [executorLoaded, setExecutorLoaded] = useState(false);
  const [deleteProfileTarget, setDeleteProfileTarget] = useState<string | null>(null);
  const [deleteVarTarget, setDeleteVarTarget] = useState<string | null>(null);
  const [deleteProviderTarget, setDeleteProviderTarget] = useState<string | null>(null);

  const [activeTab, setActiveTab] = useState<'theme' | 'env' | 'plugins' | 'executor' | 'providers'>('theme');

  const TABS = [
    { id: 'theme' as const, label: t(locale, 'settings.tabTheme') },
    { id: 'env' as const, label: t(locale, 'settings.tabEnv') },
    { id: 'plugins' as const, label: t(locale, 'settings.tabPlugins') },
    { id: 'executor' as const, label: t(locale, 'settings.tabExecutor') },
    { id: 'providers' as const, label: t(locale, 'settings.tabProviders') },
  ];

  // Providers state
  const [providers, setProviders] = useState<UserProvider[]>([]);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [providersLoading, setProvidersLoading] = useState(false);

  useEffect(() => {
    if (activeTab === 'providers') {
      loadProviders();
    }
  }, [activeTab]);

  async function loadProviders() {
    setProvidersLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.provider?.list) {
        const result = await api.provider.list() as UserProvider[];
        setProviders(Array.isArray(result) ? result : []);
      }
    } catch (e) {
      globalToast(classifyError(e).userMessage, 'error');
    } finally {
      setProvidersLoading(false);
    }
  }

  async function handleSaveProvider(data: { presetName: string; name: string; websiteUrl: string; baseUrl: string; keys: { label: string; apiKey: string }[] }) {
    const api = window.nativesAPI;
    if (!api?.provider?.add) throw new Error('Provider API not available');
    await api.provider.add(data);
    globalToast(t(locale, 'settings.providerAdded'), 'success');
    await loadProviders();
  }

  async function handleDeleteProvider(id: string) {
    const api = window.nativesAPI;
    if (!api?.provider?.delete) return;
    await api.provider.delete(id);
    globalToast(t(locale, 'settings.providerDeleted'), 'success');
    await loadProviders();
  }

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2200);
  }, []);

  // --- Plugins State ---
  const [pluginVersions, setPluginVersions] = useState<Record<string, string | null>>({
    rtk: null,
    ccusage: null,
    codegraph: null,
  });
  const [detecting, setDetecting] = useState<Record<string, boolean>>({
    rtk: false,
    ccusage: false,
    codegraph: false,
  });
  const [installing, setInstalling] = useState<Record<string, 'installing' | 'uninstalling' | 'updating' | null>>({
    rtk: null,
    ccusage: null,
    codegraph: null,
  });
  const [installLogs, setInstallLogs] = useState<Record<string, string>>({
    rtk: '',
    ccusage: '',
    codegraph: '',
  });
  const [activeLogPlugin, setActiveLogPlugin] = useState<string | null>(null);

  // 执行引擎设置：启动时从 DB 加载（PRD 3.4 持久化）
  useEffect(() => {
    (async () => {
      try {
        const api = window.nativesAPI;
        if (api?.executorSettings?.get) {
          const s = await api.executorSettings.get();
          setEnabledTools(s.enabledTools);
          setMaxSelfHeal(s.maxSelfHeal);
        }
      } catch (e) {
        globalToast(classifyError(e).userMessage, 'error');
      } finally {
        setExecutorLoaded(true);
      }
    })();
  }, [toast]);

  // 执行引擎设置：变更时保存到 DB（防抖：loaded 后才保存，避免初始化覆盖）
  const saveExecutorSettings = useCallback(async (next: { enabledTools: Record<string, boolean>; maxSelfHeal: number }) => {
    if (!executorLoaded) return;
    try {
      const api = window.nativesAPI;
      if (api?.executorSettings?.save) {
        await api.executorSettings.save(next);
      }
    } catch (e) {
      globalToast(classifyError(e).userMessage, 'error');
    }
  }, [executorLoaded, toast]);

  const toggleTool = useCallback((key: string) => {
    setEnabledTools(prev => {
      const next = { ...prev, [key]: !prev[key] };
      void saveExecutorSettings({ enabledTools: next, maxSelfHeal });
      return next;
    });
  }, [saveExecutorSettings, maxSelfHeal]);

  const updateMaxSelfHeal = useCallback((value: number) => {
    const clamped = Math.max(1, Math.min(10, value));
    setMaxSelfHeal(clamped);
    void saveExecutorSettings({ enabledTools, maxSelfHeal: clamped });
  }, [saveExecutorSettings, enabledTools]);

  const detectPlugin = useCallback(async (name: string) => {
    setDetecting(prev => ({ ...prev, [name]: true }));
    try {
      const api = window.nativesAPI;
      if (api?.plugins?.detect) {
        const version = await api.plugins.detect(name);
        setPluginVersions(prev => ({ ...prev, [name]: version }));
      }
    } catch (err) {
      globalToast(classifyError(err).userMessage, 'error');
    } finally {
      setDetecting(prev => ({ ...prev, [name]: false }));
    }
  }, [toast]);

  const detectAllPlugins = useCallback(async () => {
    await Promise.all([
      detectPlugin('rtk'),
      detectPlugin('ccusage'),
      detectPlugin('codegraph'),
    ]);
  }, [detectPlugin]);

  useEffect(() => {
    if (activeTab === 'plugins') {
      detectAllPlugins();
    }
  }, [activeTab, detectAllPlugins]);

  // Bind Tauri install/uninstall logs and complete events
  useEffect(() => {
    let unlistenLog: (() => void) | undefined;
    let unlistenInstallComplete: (() => void) | undefined;
    let unlistenUninstallComplete: (() => void) | undefined;

    async function setupListeners() {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        
        const unsubLog = await listen<{ name: string; log: string }>('plugin:install-log', (event) => {
          const { name, log } = event.payload;
          setInstallLogs(prev => ({
            ...prev,
            [name]: (prev[name] || '') + log
          }));
        });
        unlistenLog = unsubLog;

        const unsubInstall = await listen<{ name: string; success: boolean; error?: string }>('plugin:install-complete', (event) => {
          const { name, success, error } = event.payload;
          setInstalling(prev => ({ ...prev, [name]: null }));
          if (success) {
            showToast(t(locale, 'settings.plugins.success').replace('{name}', name));
          } else {
            showToast(t(locale, 'settings.plugins.failed').replace('{name}', name).replace('{error}', error || ''));
          }
          detectPlugin(name);
        });
        unlistenInstallComplete = unsubInstall;

        const unsubUninstall = await listen<{ name: string; success: boolean; error?: string }>('plugin:uninstall-complete', (event) => {
          const { name, success, error } = event.payload;
          setInstalling(prev => ({ ...prev, [name]: null }));
          if (success) {
            showToast(t(locale, 'settings.plugins.success').replace('{name}', name));
          } else {
            showToast(t(locale, 'settings.plugins.failed').replace('{name}', name).replace('{error}', error || ''));
          }
          detectPlugin(name);
        });
        unlistenUninstallComplete = unsubUninstall;
      } catch (err) {
        globalToast(classifyError(err).userMessage, 'error');
      }
    }

    setupListeners();

    return () => {
      if (unlistenLog) unlistenLog();
      if (unlistenInstallComplete) unlistenInstallComplete();
      if (unlistenUninstallComplete) unlistenUninstallComplete();
    };
  }, [detectPlugin, locale, showToast]);

  const handleInstallPlugin = useCallback(async (name: string, isUpdate = false) => {
    setInstalling(prev => ({ ...prev, [name]: isUpdate ? 'updating' : 'installing' }));
    setInstallLogs(prev => ({ ...prev, [name]: '' }));
    setActiveLogPlugin(name);
    try {
      const api = window.nativesAPI;
      if (api?.plugins?.install) {
        await api.plugins.install(name);
      }
    } catch (err) {
      const classified = classifyError(err);
      showToast(t(locale, 'settings.plugins.failed').replace('{name}', name).replace('{error}', classified.userMessage));
      setInstalling(prev => ({ ...prev, [name]: null }));
    }
  }, [locale, showToast, t]);

  const handleUninstallPlugin = useCallback(async (name: string) => {
    setInstalling(prev => ({ ...prev, [name]: 'uninstalling' }));
    setInstallLogs(prev => ({ ...prev, [name]: '' }));
    setActiveLogPlugin(name);
    try {
      const api = window.nativesAPI;
      if (api?.plugins?.uninstall) {
        await api.plugins.uninstall(name);
      }
    } catch (err) {
      const classified = classifyError(err);
      showToast(t(locale, 'settings.plugins.failed').replace('{name}', name).replace('{error}', classified.userMessage));
      setInstalling(prev => ({ ...prev, [name]: null }));
    }
  }, [locale, showToast, t]);

  // Load persisted settings on mount
  useEffect(() => { // eslint-disable-line react-hooks/rules-of-hooks
    async function loadSettings() {
      try {
        const api = window.nativesAPI;
        if (!api) return;
        const [savedTheme, savedLocale] = await Promise.all([
          api.getTheme().catch(() => null),
          api.getLocale().catch(() => null),
        ]);
        if (savedTheme) {
          const normalizedTheme = normalizeThemeId(savedTheme);
          setThemeState(normalizedTheme);
          applyTheme(normalizedTheme);
        }
        if (savedLocale) setLocaleState(savedLocale as Locale);

        const db = api.db;
        if (db?.get) {
          const [sw, pw, th] = await Promise.all([
            db.get('settings:sidebar_width').catch(() => null),
            db.get('settings:panel_width').catch(() => null),
            db.get('settings:terminal_height').catch(() => null),
          ]);
          if (sw) setSidebarWidth(Number(sw));
          if (pw) setPanelWidth(Number(pw));
          if (th) setTerminalHeight(Number(th));
        }
      } catch (_e) { /* browser dev mode */ }
    }
    loadSettings();
  }, []);

  // Load environment profiles
  const loadProfiles = useCallback(async () => {
    try {
      const api = window.nativesAPI;
      if (!api?.env) return;
      const list = await api.env.listProfiles();
      setProfiles(list as unknown as EnvProfile[]);
      // Auto-select first profile if none selected
      if (!selectedProfile && (list as unknown as EnvProfile[]).length > 0) {
        setSelectedProfile((list as unknown as EnvProfile[])[0]!.name);
      }
    } catch (_e) { /* browser dev mode */ }
  }, [selectedProfile]);

  // Load variables for selected profile
  const loadVariables = useCallback(async (profileName: string) => {
    try {
      const api = window.nativesAPI;
      if (!api?.env) return;
      const vars = await api.env.getVariables(profileName);
      setVariables(
        Object.entries(vars).map(([key, value]) => ({ key, value: String(value) }))
      );
    } catch (_e) { /* browser dev mode */ }
  }, []);

  useEffect(() => { // eslint-disable-line react-hooks/rules-of-hooks
    loadProfiles();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => { // eslint-disable-line react-hooks/rules-of-hooks
    if (selectedProfile) {
      loadVariables(selectedProfile);
    }
  }, [selectedProfile, loadVariables]);

  const THEMES = [
    { id: 'dark', label: t(locale, 'settings.themeTerminal'), desc: t(locale, 'settings.themeDescTerminal') },
    { id: 'light', label: t(locale, 'settings.themeJasmine'), desc: t(locale, 'settings.themeDescJasmine') },
  ];

  const LOCALES = [
    { id: 'zh-CN', label: '中文' },
    { id: 'en', label: 'English' },
  ];

  const handleThemeChange = (themeId: string) => {
    const normalizedTheme = normalizeThemeId(themeId);
    setThemeState(normalizedTheme);
    applyTheme(normalizedTheme);
    try { window.nativesAPI?.setTheme?.(normalizedTheme); } catch (_e) { /* browser dev mode */ }
  };

  const handleLocaleChange = async (localeId: string) => {
    setLocaleState(localeId as Locale);
    /* eslint-disable-next-line no-var, prefer-const */
    document.documentElement.lang = localeId;
    try {
      await window.nativesAPI?.setLocale?.(localeId);
      // Notify all locale-aware components to refresh
      window.dispatchEvent(new CustomEvent('locale-changed', { detail: localeId }));
    } catch (_e) { /* browser dev mode */ }
  };

  const saveLayoutSetting = (key: string, value: number) => {
    try { window.nativesAPI?.db?.set?.(`settings:${key}`, String(value)); } catch (_e) { /* browser dev mode */ }
  };

  // Profile CRUD
  const handleCreateProfile = async () => {
    const name = newProfileName.trim();
    if (!name) return;
    try {
      await window.nativesAPI?.env?.createProfile(name);
      setNewProfileName('');
      setShowNewProfile(false);
      setSelectedProfile(name);
      await loadProfiles();
      showToast(t(locale, 'settings.profileCreated'));
    } catch (err) {
      globalToast(t(locale, 'settings.profileCreated') + ' ' + t(locale, 'common.error'), 'error');
    }
  };

  const doDeleteProfile = async (name: string) => {
    try {
      await window.nativesAPI?.env?.deleteProfile(name);
      if (selectedProfile === name) {
        setSelectedProfile(null);
        setVariables([]);
      }
      await loadProfiles();
      showToast(t(locale, 'settings.profileDeleted'));
    } catch (err) {
      globalToast(t(locale, 'settings.profileDeleted') + ' ' + t(locale, 'common.error'), 'error');
    }
  };

  const handleDeleteProfile = (name: string) => {
    setDeleteProfileTarget(name);
  };

  const handleConfirmDeleteProfile = async () => {
    if (!deleteProfileTarget) return;
    await doDeleteProfile(deleteProfileTarget);
    setDeleteProfileTarget(null);
  };

  // Variable CRUD
  const handleAddVariable = async () => {
    const key = newVarKey.trim();
    const value = newVarValue;
    if (!key || !selectedProfile) return;
    try {
      await window.nativesAPI?.env?.setVariable(selectedProfile, key, value);
      setNewVarKey('');
      setNewVarValue('');
      setShowNewVar(false);
      await loadVariables(selectedProfile);
      showToast(t(locale, 'settings.variableSaved'));
    } catch (err) {
      globalToast(classifyError(err).userMessage, 'error');
    }
  };

  const handleDeleteVariable = (key: string) => {
    setDeleteVarTarget(key);
  };

  const handleConfirmDeleteVariable = async () => {
    if (!deleteVarTarget || !selectedProfile) return;
    try {
      await window.nativesAPI?.env?.deleteVariable?.(selectedProfile, deleteVarTarget);
      if (selectedProfile) await loadVariables(selectedProfile);
      showToast(t(locale, 'settings.variableDeleted'));
    } catch (err) {
      globalToast(classifyError(err).userMessage, 'error');
    } finally {
      setDeleteVarTarget(null);
    }
  };

  const handleEditVariable = async (key: string) => {
    if (!selectedProfile) return;
    try {
      await window.nativesAPI?.env?.setVariable(selectedProfile, key, editValue);
      setEditingVar(null);
      setEditValue('');
      await loadVariables(selectedProfile);
      showToast(t(locale, 'settings.variableSaved'));
    } catch (err) {
      globalToast(classifyError(err).userMessage, 'error');
    }
  };

  const maskValue = (value: string) => {
    if (value.length <= 4) return '••••';
    return value.slice(0, 2) + '•'.repeat(Math.min(value.length - 4, 20)) + value.slice(-2);
  };

  return (
    <div style={{ height: '100%', overflow: 'auto', position: 'relative' }}>
      <div style={{ padding: `${SPACING.lg}px 20px`, display: 'flex', flexDirection: 'column', gap: 28 }}>
        {/* Tab Bar */}
        <div style={{ display: 'flex', gap: 4, background: 'var(--surface-hover)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', padding: 3 }}>
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              style={{
                flex: 1, textAlign: 'center', padding: '7px 4px',
                borderRadius: 'calc(var(--radius-sm) - 2px)', cursor: 'pointer',
                background: activeTab === tab.id ? 'var(--primary-soft)' : 'transparent',
                color: activeTab === tab.id ? 'var(--primary)' : 'var(--text-secondary)',
                border: activeTab === tab.id ? '1px solid var(--primary)' : '1px solid transparent',
                fontSize: FONT_SIZE.md, fontWeight: activeTab === tab.id ? 600 : 400,
                transition: 'all 0.12s ease-out',
              }}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Tab 1: 主题与样式 */}
        <div style={{ display: activeTab === 'theme' ? 'flex' : 'none', flexDirection: 'column', gap: 28 }}>
          {/* Theme */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.theme')}
          </h2>
          <div style={{ display: 'flex', gap: SPACING.sm }}>
            {THEMES.map((th) => (
              <button
                key={th.id}
                className={`btn ${theme === th.id ? 'btn-primary' : ''}`}
                onClick={() => handleThemeChange(th.id)}
                style={{ flex: 1, textAlign: 'center', padding: '10px 8px' }}
              >
                <div style={{ fontWeight: 600, fontSize: FONT_SIZE.md }}>{th.label}</div>
                <div style={{ fontSize: FONT_SIZE.xs, color: theme === th.id ? '#FFFFFF' : 'var(--text-disabled)', marginTop: SPACING.xs }}>{th.desc}</div>
              </button>
            ))}
          </div>
        </section>

        {/* Language */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.language')}
          </h2>
          <div style={{ display: 'flex', gap: SPACING.sm }}>
            {LOCALES.map((l) => (
              <button
                key={l.id}
                className={`btn ${locale === l.id ? 'btn-primary' : ''}`}
                onClick={() => handleLocaleChange(l.id)}
                style={{ flex: 1, textAlign: 'center' }}
              >
                {l.label}
              </button>
            ))}
          </div>
        </section>

        {/* Layout */}
        <div style={SectionCardOuter}>
        <div style={SectionCardInner}>
        <div style={sectionCardStyle}>
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.layout')}
          </h2>
          <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.md }}>
            {[
              { label: t(locale, 'settings.sidebarWidth'), value: sidebarWidth, key: 'sidebar_width', set: setSidebarWidth, min: 190, max: 420 },
              { label: t(locale, 'settings.panelWidth'), value: panelWidth, key: 'panel_width', set: setPanelWidth, min: 200, max: 600 },
              { label: t(locale, 'settings.terminalHeight'), value: terminalHeight, key: 'terminal_height', set: setTerminalHeight, min: 100, max: 600 },
            ].map((item) => (
              <div key={item.key} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <label style={{ fontSize: FONT_SIZE.md, color: 'var(--text)' }}>{item.label}</label>
                <input
                  type="number"
                  value={item.value}
                  min={item.min}
                  max={item.max}
                  style={inputStyle}
                  onChange={(e) => {
                    const v = Number(e.target.value);
                    item.set(v);
                    saveLayoutSetting(item.key, v);
                  }}
                />
              </div>
            ))}
          </div>
        </section>
        </div>
        </div>
        </div>

        {/* About */}
        <div style={SectionCardOuter}>
        <div style={SectionCardInner}>
        <div style={sectionCardStyle}>
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.about')}
          </h2>
          <p style={{ fontSize: FONT_SIZE.md, color: 'var(--text-disabled)' }}>
            {t(locale, 'settings.aboutVersion')}
          </p>
        </section>
        </div>
        </div>
        </div>
        </div>
        {/* ⇡ Tab 1 end ⇡ */}

        {/* Tab 3: 环境配置 */}
        <div style={{ display: activeTab === 'env' ? 'flex' : 'none', flexDirection: 'column', gap: 28 }}>
          {/* Environment Profiles */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.environment')}
          </h2>

          {profiles.length === 0 && !showNewProfile ? (
            <div style={{
              padding: `${SPACING.xl}px ${SPACING.lg}px`,
              textAlign: 'center',
              border: '1px dashed var(--border)',
              borderRadius: 'var(--radius-md)',
              color: 'var(--text-secondary)',
            }}>
              <div style={{ fontSize: FONT_SIZE.lg, marginBottom: SPACING.xs }}>{t(locale, 'settings.noProfiles')}</div>
              <div style={{ fontSize: FONT_SIZE.sm, marginBottom: SPACING.md }}>{t(locale, 'settings.noProfilesDesc')}</div>
              <button className="btn btn-primary" onClick={() => setShowNewProfile(true)}>
                + {t(locale, 'settings.addProfile')}
              </button>
            </div>
          ) : (
            <>
              {/* Profile list */}
              <div style={SectionCardOuter}>
              <div style={SectionCardInner}>
              <div style={{ ...sectionCardStyle, marginBottom: SPACING.md }}>
                <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.xs }}>
                  {profiles.map((p) => (
                    <div
                      key={p.id}
                      onClick={() => setSelectedProfile(p.name)}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        padding: `${SPACING.sm}px 10px`,
                        borderRadius: 'calc(var(--radius-md) - 2px)',
                        cursor: 'pointer',
                        background: selectedProfile === p.name ? 'var(--primary-soft)' : 'transparent',
                        border: selectedProfile === p.name ? '1px solid var(--primary)' : '1px solid transparent',
                        transition: 'all 0.12s',
                      }}
                    >
                      <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm }}>
                        <span style={{ fontSize: FONT_SIZE.lg, color: 'var(--text)', fontWeight: selectedProfile === p.name ? 600 : 400 }}>
                          {p.name}
                        </span>
                        {p.is_default === 1 && (
                          <span style={{
                            fontSize: 9,
                            padding: '1px 5px',
                            borderRadius: 'calc(var(--radius-md) - 4px)',
                            background: 'var(--primary)',
                            color: '#FFFFFF',
                            fontWeight: 600,
                            textTransform: 'uppercase',
                            letterSpacing: 0.5,
                          }}>
                            {t(locale, 'settings.defaultProfile')}
                          </span>
                        )}
                      </div>
                      <div style={{ display: 'flex', gap: SPACING.xs }}>
                        <button
                          className="btn"
                          style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px' }}
                          onClick={async (e) => {
                            e.stopPropagation();
                            try {
                              await window.nativesAPI?.env?.setDefaultProfile?.(p.name);
                              await loadProfiles();
                              showToast(t(locale, 'settings.defaultSet'));
                            } catch (_e) {
                              showToast(t(locale, 'common.error'));
                            }
                          }}
                          title={t(locale, 'settings.setDefault')}
                        >
                          <Star size={12} />
                        </button>
                        <button
                          className="btn"
                          style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', color: 'var(--danger)' }}
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDeleteProfile(p.name);
                          }}
                          title={t(locale, 'settings.deleteProfile')}
                        >
                          <X size={12} />
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
              </div>
              </div>

              {/* Add profile inline */}
              {showNewProfile ? (
                <div style={{ display: 'flex', gap: 6, marginBottom: SPACING.md }}>
                  <input
                    type="text"
                    value={newProfileName}
                    onChange={(e) => setNewProfileName(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleCreateProfile()}
                    placeholder={t(locale, 'settings.profileNamePlaceholder')}
                    style={{ ...inputStyle, flex: 1 }}
                    autoFocus
                  />
                  <button className="btn btn-primary" onClick={handleCreateProfile} style={{ fontSize: FONT_SIZE.md }}>
                    {t(locale, 'common.confirm')}
                  </button>
                  <button className="btn" onClick={() => { setShowNewProfile(false); setNewProfileName(''); }} style={{ fontSize: FONT_SIZE.md }}>
                    {t(locale, 'common.cancel')}
                  </button>
                </div>
              ) : (
                <button className="btn" style={{ width: '100%', marginBottom: SPACING.md }} onClick={() => setShowNewProfile(true)}>
                  + {t(locale, 'settings.addProfile')}
                </button>
              )}

              {/* Variables for selected profile */}
              {selectedProfile && (
                <div style={{
                  border: '1px solid var(--border)',
                  borderRadius: 'var(--radius-md)',
                  overflow: 'hidden',
                }}>
                  <div style={{
                    padding: `${SPACING.sm}px 10px`,
                    background: 'var(--surface)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    borderBottom: '1px solid var(--border)',
                  }}>
                    <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 1 }}>
                      {t(locale, 'settings.variables')}
                    </span>
                    <button
                      className="btn"
                      style={{ fontSize: FONT_SIZE.xs, padding: '2px 8px' }}
                      onClick={() => setShowNewVar(true)}
                    >
                      + {t(locale, 'settings.addVariable')}
                    </button>
                  </div>

                  {/* Variable list */}
                  <div style={{ maxHeight: 240, overflow: 'auto' }}>
                    {variables.filter(v => v.value !== '').length === 0 && !showNewVar ? (
                      <div style={{ padding: `${SPACING.lg}px 10px`, textAlign: 'center', color: 'var(--text-disabled)', fontSize: FONT_SIZE.md }}>
                        {t(locale, 'settings.noVariables')}
                      </div>
                    ) : (
                      variables.filter(v => v.value !== '').map((v) => (
                        <div
                          key={v.key}
                          style={{
                            padding: '6px 10px',
                            borderBottom: '1px solid var(--border)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: SPACING.sm,
                          }}
                        >
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{ fontSize: FONT_SIZE.md, fontWeight: 600, color: 'var(--primary)', fontFamily: 'var(--font-mono)' }}>
                              {v.key}
                            </div>
                            {editingVar === v.key ? (
                              <div style={{ display: 'flex', gap: SPACING.xs, marginTop: SPACING.xs }}>
                                <input
                                  type="password"
                                  value={editValue}
                                  onChange={(e) => setEditValue(e.target.value)}
                                  onKeyDown={(e) => e.key === 'Enter' && handleEditVariable(v.key)}
                                  style={{ ...inputStyle, flex: 1, fontSize: FONT_SIZE.sm, padding: `${SPACING.xs}px 6px` }}
                                  autoFocus
                                />
                                <button className="btn btn-primary" style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px' }} onClick={() => handleEditVariable(v.key)}>
                                  <Check size={12} />
                                </button>
                                <button className="btn" style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px' }} onClick={() => { setEditingVar(null); setEditValue(''); }}>
                                  <X size={12} />
                                </button>
                              </div>
                            ) : (
                              <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-disabled)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
                                {maskValue(v.value)}
                              </div>
                            )}
                          </div>
                          <div style={{ display: 'flex', gap: 2, flexShrink: 0 }}>
                            <button
                              className="btn"
                              style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px' }}
                              onClick={() => { setEditingVar(v.key); setEditValue(v.value); }}
                              title={t(locale, 'common.edit')}
                            >
                              <Edit2 size={12} />
                            </button>
                            <button
                              className="btn"
                              style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', color: 'var(--danger)' }}
                              onClick={() => handleDeleteVariable(v.key)}
                              title={t(locale, 'common.delete')}
                            >
                              <X size={12} />
                            </button>
                          </div>
                        </div>
                      ))
                    )}

                    {/* Add variable inline */}
                    {showNewVar && (
                      <div style={{ padding: `${SPACING.sm}px 10px`, display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <input
                          type="text"
                          value={newVarKey}
                          onChange={(e) => setNewVarKey(e.target.value)}
                          placeholder={t(locale, 'settings.variableKeyPlaceholder')}
                          style={{ ...inputStyle, fontSize: FONT_SIZE.md, padding: '6px 8px' }}
                          autoFocus
                        />
                        <input
                          type="password"
                          value={newVarValue}
                          onChange={(e) => setNewVarValue(e.target.value)}
                          onKeyDown={(e) => e.key === 'Enter' && handleAddVariable()}
                          placeholder={t(locale, 'settings.variableValuePlaceholder')}
                          style={{ ...inputStyle, fontSize: FONT_SIZE.md, padding: '6px 8px' }}
                        />
                        <div style={{ display: 'flex', gap: 6 }}>
                          <button className="btn btn-primary" onClick={handleAddVariable} style={{ fontSize: FONT_SIZE.sm, flex: 1 }}>
                            {t(locale, 'common.save')}
                          </button>
                          <button className="btn" onClick={() => { setShowNewVar(false); setNewVarKey(''); setNewVarValue(''); }} style={{ fontSize: FONT_SIZE.sm }}>
                            {t(locale, 'common.cancel')}
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </>
          )}
        </section>
        </div>
        {/* ⇡ Tab 3 end ⇡ */}

        {/* Tab 4: 插件设置 */}
        <div style={{ display: activeTab === 'plugins' ? 'flex' : 'none', flexDirection: 'column', gap: 28 }}>
          <div style={SectionCardOuter}>
          <div style={SectionCardInner}>
          <section style={sectionCardStyle}>
            <div style={sectionTitleStyle}>
              {t(locale, 'settings.tabPlugins')}
            </div>
            
            {/* V1.0 Plugins Table — 纯色 Surface */}
            <div style={{
              overflowX: 'auto',
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--border)',
              background: 'var(--surface)',
            }}>
              <table style={{
                width: '100%',
                borderCollapse: 'collapse',
                textAlign: 'left',
                fontSize: FONT_SIZE.md,
                color: 'var(--text)',
              }}>
                <thead>
                  <tr style={{
                    borderBottom: '1px solid var(--border)',
                    background: 'rgba(255, 255, 255, 0.03)',
                  }}>
                    <th style={{ padding: '12px 16px', fontWeight: 600, color: 'var(--text-secondary)' }}>
                      {t(locale, 'settings.plugins.name')}
                    </th>
                    <th style={{ padding: '12px 16px', fontWeight: 600, color: 'var(--text-secondary)' }}>
                      {t(locale, 'settings.plugins.description')}
                    </th>
                    <th style={{ padding: '12px 16px', fontWeight: 600, color: 'var(--text-secondary)' }}>
                      {t(locale, 'settings.plugins.version')}
                    </th>
                    <th style={{ padding: '12px 16px', fontWeight: 600, color: 'var(--text-secondary)', textAlign: 'right' }}>
                      {t(locale, 'settings.plugins.actions')}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {[
                    {
                      id: 'rtk',
                      name: 'RTK',
                      url: 'https://github.com/rtk-ai/rtk',
                      desc: locale === 'zh' 
                        ? 'AI Native CLI 辅助工具，用于节省 Token 并追踪执行过程。' 
                        : 'AI Native CLI helper for token savings and execution tracing.',
                    },
                    {
                      id: 'ccusage',
                      name: 'ccusage',
                      url: 'https://github.com/ccusage/ccusage',
                      desc: locale === 'zh'
                        ? 'AI 编码助手的 Token 使用量及估算成本计算器。'
                        : 'Token usage and estimated cost calculator for AI coding agents.',
                    },
                    {
                      id: 'codegraph',
                      name: 'CodeGraph',
                      url: 'https://github.com/colbymchenry/codegraph',
                      desc: locale === 'zh'
                        ? '代码结构浏览器与项目依赖关系可视化分析器。'
                        : 'Code structure explorer and dependency relationships visualizer.',
                    }
                  ].map((plugin) => {
                    const version = pluginVersions[plugin.id];
                    const isDetecting = detecting[plugin.id];
                    const installStatus = installing[plugin.id];
                    const isBusy = isDetecting || !!installStatus;
                    const isInstalled = version !== null;

                    return (
                      <tr key={plugin.id} style={{
                        borderBottom: '1px solid var(--border)',
                        background: activeLogPlugin === plugin.id ? 'rgba(255, 255, 255, 0.02)' : 'transparent',
                        transition: 'background 0.2s ease',
                      }}>
                        {/* Name */}
                        <td style={{ padding: '14px 16px', fontWeight: 600 }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                            <span>{plugin.name}</span>
                            <a 
                              href={plugin.url} 
                              target="_blank" 
                              rel="noreferrer"
                              title={plugin.url}
                              style={{ display: 'inline-flex', color: 'var(--text-secondary)' }}
                            >
                              <ExternalLink size={12} />
                            </a>
                          </div>
                        </td>
                        
                        {/* Description */}
                        <td style={{ padding: '14px 16px', color: 'var(--text-secondary)', maxWidth: '400px' }}>
                          {plugin.desc}
                        </td>

                        {/* Version */}
                        <td style={{ padding: '14px 16px' }}>
                          {isDetecting ? (
                            <span style={{ color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm }}>
                              {t(locale, 'common.loading') || '检测中...'}
                            </span>
                          ) : isInstalled ? (
                            <span style={{
                              background: 'var(--primary-soft)',
                              color: 'var(--primary)',
                              padding: '2px 8px',
                              borderRadius: '12px',
                              fontSize: FONT_SIZE.sm,
                              fontWeight: 600,
                              border: '1px solid var(--primary)',
                            }}>
                              v{version}
                            </span>
                          ) : (
                            <span style={{
                              color: 'var(--text-secondary)',
                              fontSize: FONT_SIZE.sm,
                              fontStyle: 'italic'
                            }}>
                              {t(locale, 'settings.plugins.notInstalled')}
                            </span>
                          )}
                        </td>

                        {/* Actions */}
                        <td style={{ padding: '14px 16px', textAlign: 'right' }}>
                          <div style={{ display: 'inline-flex', gap: 8, alignItems: 'center' }}>
                            {!isInstalled ? (
                              <button
                                disabled={isBusy}
                                onClick={() => handleInstallPlugin(plugin.id)}
                                className="btn btn-primary"
                                style={{
                                  padding: '5px 12px',
                                  fontSize: FONT_SIZE.sm,
                                  opacity: isBusy ? 0.6 : 1,
                                  cursor: isBusy ? 'not-allowed' : 'pointer',
                                }}
                              >
                                {installStatus === 'installing' ? t(locale, 'settings.plugins.installing') : t(locale, 'settings.plugins.install')}
                              </button>
                            ) : (
                              <>
                                <button
                                  disabled={isBusy}
                                  onClick={() => handleInstallPlugin(plugin.id, true)}
                                  className="btn"
                                  style={{
                                    padding: '5px 12px',
                                    fontSize: FONT_SIZE.sm,
                                    borderColor: 'var(--border)',
                                    color: 'var(--text)',
                                    opacity: isBusy ? 0.6 : 1,
                                    cursor: isBusy ? 'not-allowed' : 'pointer',
                                  }}
                                >
                                  {installStatus === 'updating' ? t(locale, 'settings.plugins.updating') : t(locale, 'settings.plugins.update')}
                                </button>
                                <button
                                  disabled={isBusy}
                                  onClick={() => handleUninstallPlugin(plugin.id)}
                                  className="btn"
                                  style={{
                                    padding: '5px 12px',
                                    fontSize: FONT_SIZE.sm,
                                    background: 'rgba(239, 68, 68, 0.1)',
                                    border: '1px solid rgba(239, 68, 68, 0.2)',
                                    borderRadius: 'calc(var(--radius-md) - 6px)',
                                    color: 'rgba(239, 68, 68, 0.9)',
                                    cursor: isBusy ? 'not-allowed' : 'pointer',
                                    opacity: isBusy ? 0.6 : 1,
                                  }}
                                >
                                  {installStatus === 'uninstalling' ? t(locale, 'settings.plugins.uninstalling') : t(locale, 'settings.plugins.uninstall')}
                                </button>
                              </>
                            )}

                            {/* Log expansion trigger */}
                            {installLogs[plugin.id] && (
                              <button
                                onClick={() => setActiveLogPlugin(activeLogPlugin === plugin.id ? null : plugin.id)}
                                style={{
                                  background: 'none',
                                  border: 'none',
                                  color: 'var(--text-secondary)',
                                  cursor: 'pointer',
                                  padding: '4px',
                                  display: 'flex',
                                  alignItems: 'center',
                                }}
                                title={t(locale, 'settings.viewLog')}
                              >
                                <Terminal size={14} style={{ color: activeLogPlugin === plugin.id ? 'var(--primary)' : 'inherit' }} />
                              </button>
                            )}
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>

            {/* Collapsible Log Card */}
            {activeLogPlugin && installLogs[activeLogPlugin] && (
              <div style={{
                marginTop: 12,
                borderRadius: 'calc(var(--radius-md) - 2px)',
                border: '1px solid var(--border)',
                background: 'rgba(0, 0, 0, 0.25)',
                boxShadow: 'inset 0 1px 4px rgba(0, 0, 0, 0.3)',
                padding: '12px 16px',
                display: 'flex',
                flexDirection: 'column',
                gap: 8,
              }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--text)', fontSize: FONT_SIZE.sm, fontWeight: 600 }}>
                    <Terminal size={14} className="text-[var(--primary)]" />
                    <span>{t(locale, 'settings.plugins.consoleTitle')} - {activeLogPlugin.toUpperCase()}</span>
                  </div>
                  <button 
                    onClick={() => setActiveLogPlugin(null)}
                    style={{ background: 'none', border: 'none', color: 'var(--text-secondary)', cursor: 'pointer', fontSize: FONT_SIZE.sm }}
                  >
                    {t(locale, 'common.close') || '关闭'}
                  </button>
                </div>
                <pre style={{
                  margin: 0,
                  padding: '8px 10px',
                  background: 'rgba(0, 0, 0, 0.3)',
                  borderRadius: 4,
                  fontFamily: 'monospace',
                  fontSize: '12px',
                  color: '#10b981',
                  overflowY: 'auto',
                  maxHeight: '220px',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-all',
                  textAlign: 'left',
                }}>
                  {installLogs[activeLogPlugin]}
                </pre>
              </div>
            )}
          </section>
          </div>
          </div>
        </div>

        {/* Tab 5: 执行引擎 (Execution Engine) — RuntimePanel */}
        <div style={{ display: activeTab === 'executor' ? 'block' : 'none' }}>
          <RuntimePanel locale={locale} />
        </div>

        {/* Tab 6: 供应商管理 (Providers) */}
        <div style={{ display: activeTab === 'providers' ? 'block' : 'none' }}>
          <section>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: SPACING.md }}>
              <h2 style={sectionTitleStyle}>
                {t(locale, 'settings.providers')}
              </h2>
              <button
                onClick={() => setShowAddProvider(true)}
                style={{
                  display: 'inline-flex', alignItems: 'center', gap: 4,
                  padding: '6px 12px', borderRadius: BORDER_RADIUS.md,
                  background: 'var(--primary)', border: 'none',
                  color: '#FFFFFF', fontWeight: 600, cursor: 'pointer',
                  fontSize: FONT_SIZE.sm,
                }}
              >
                  {t(locale, 'settings.addProvider')}
              </button>
            </div>

            {providersLoading ? (
              <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-disabled)' }}>
                {t(locale, 'settings.providersLoading')}
              </div>
            ) : providers.length === 0 ? (
              <div style={{
                padding: SPACING.xxl, textAlign: 'center', color: 'var(--text-disabled)',
                border: '1px dashed var(--border)', borderRadius: BORDER_RADIUS.lg,
              }}>
                <div style={{ fontSize: FONT_SIZE.sm, marginBottom: SPACING.sm }}>
                  {t(locale, 'settings.noProviders')}
                </div>
                <button
                  onClick={() => setShowAddProvider(true)}
                  style={{
                    padding: '6px 14px', borderRadius: BORDER_RADIUS.md,
                    background: 'var(--primary)', border: 'none',
                    color: '#FFFFFF', cursor: 'pointer', fontSize: FONT_SIZE.sm,
                  }}
                >
                  {t(locale, 'settings.addProvider')}
                </button>
              </div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.sm }}>
                {providers.map((p) => (
                  <div
                    key={p.id}
                    style={{
                      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                      padding: `${SPACING.md}px ${SPACING.lg}px`,
                      borderRadius: BORDER_RADIUS.lg,
                      border: '1px solid var(--border)',
                      background: 'var(--surface)',
                    }}
                  >
                    <div>
                      <div style={{ fontWeight: 600, fontSize: FONT_SIZE.md, color: 'var(--text)' }}>
                        {p.name}
                      </div>
                      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
                        {p.baseUrl || p.websiteUrl}
                      </div>
                      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', marginTop: 4 }}>
                        {t(locale, 'settings.providerKeyCount', { count: (p.keys || []).length })}
                        {(p.keys || []).slice(0, 3).map(k => (
                          <span key={k.id} style={{ marginLeft: 8, fontSize: '0.625rem', color: 'var(--text-disabled)' }}>
                            {k.label}: {k.maskedKey}
                          </span>
                        ))}
                        {(p.keys || []).length > 3 && (
                          <span style={{ marginLeft: 4, color: 'var(--text-disabled)', fontSize: '0.625rem' }}>
                            {t(locale, 'settings.providerMoreKeys', { count: p.keys.length - 3 })}
                          </span>
                        )}
                      </div>
                    </div>
                    <button
                      onClick={() => setDeleteProviderTarget(p.id)}
                      style={{
                        background: 'none', border: 'none', color: 'var(--danger)',
                        cursor: 'pointer', padding: 6,
                      }}
                      title={t(locale, 'settings.deleteProvider')}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      </div>

      {/* Confirm dialogs */}
      <ConfirmDialog
        open={deleteProfileTarget !== null}
        title={t(locale, 'settings.confirmDeleteProfile')}
        message={t(locale, 'settings.confirmDeleteProfile')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={handleConfirmDeleteProfile}
        onCancel={() => setDeleteProfileTarget(null)}
      />
      <ConfirmDialog
        open={deleteVarTarget !== null}
        title={t(locale, 'settings.confirmDeleteVariable')}
        message={t(locale, 'settings.confirmDeleteVariable')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={handleConfirmDeleteVariable}
        onCancel={() => setDeleteVarTarget(null)}
      />

      {/* Add Provider Dialog */}
      {showAddProvider && (
        <AddProviderDialog
          locale={locale}
          onClose={() => setShowAddProvider(false)}
          onSave={handleSaveProvider}
        />
      )}

      {/* Delete Provider confirmation */}
      <ConfirmDialog
        open={deleteProviderTarget !== null}
        title={t(locale, 'settings.deleteProvider')}
        message={t(locale, 'settings.confirmDeleteProvider')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => {
          if (deleteProviderTarget) handleDeleteProvider(deleteProviderTarget);
          setDeleteProviderTarget(null);
        }}
        onCancel={() => setDeleteProviderTarget(null)}
      />

      {/* Toast */}
      {toast && (
        <div style={{
          position: 'fixed',
          bottom: SPACING.xl,
          left: '50%',
          transform: 'translateX(-50%)',
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          padding: '10px 18px',
          borderRadius: 'var(--radius-lg)',
          fontSize: FONT_SIZE.sm,
          color: 'var(--text)',
          zIndex: 200,
          animation: 'fadeIn 150ms ease',
          boxShadow: 'var(--shadow-popup)',
        }}>
          {toast}
        </div>
      )}
    </div>
  );
}

const sectionTitleStyle: React.CSSProperties = {
  fontSize: FONT_SIZE.lg,
  fontWeight: 600,
  color: 'var(--text-secondary)',
  marginBottom: SPACING.sm,
  textTransform: 'uppercase',
  letterSpacing: 1,
};

const inputStyle: React.CSSProperties = {
  width: 80,
  padding: `${SPACING.xs}px ${SPACING.sm}px`,
  background: 'var(--surface)',
  border: '1px solid var(--border)',
  borderRadius: 'calc(var(--radius-md) - 4px)',
  color: 'var(--text)',
  fontSize: FONT_SIZE.md,
};

// ── Section Card — Doppelrand for settings panels ──
const SectionCardOuter: React.CSSProperties = {
  background: 'rgba(255,255,255,0.05)',
  border: '1px solid rgba(255,255,255,0.10)',
  borderRadius: '2rem',
  padding: '0.5rem',
};

const SectionCardInner: React.CSSProperties = {
  background: 'var(--surface)',
  borderRadius: 'calc(2rem - 0.5rem)',
  boxShadow: 'inset 0 1px 1px rgba(255,255,255,0.15)',
  overflow: 'hidden',
};

const sectionCardStyle: React.CSSProperties = {
  background: 'var(--surface)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-md)',
  padding: `${SPACING.md}px ${SPACING.lg}px`,
  display: 'flex',
  flexDirection: 'column',
  gap: SPACING.md,
};
