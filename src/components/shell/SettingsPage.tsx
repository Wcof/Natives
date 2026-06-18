'use client';

import { useState, useEffect, useCallback } from 'react';
import { applyTheme } from '@/lib/theme-engine';
import { applyLiquidGlassConfig } from '@/context/ThemeContext';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { X, Check, Star, Edit2 } from 'lucide-react';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';

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
  const [theme, setThemeState] = useState('terminal-volt');
  const [locale, setLocaleState] = useState<Locale>('zh');
  const [sidebarWidth, setSidebarWidth] = useState(248);
  const [panelWidth, setPanelWidth] = useState(320);
  const [terminalHeight, setTerminalHeight] = useState(280);

  // Liquid Glass visual config (shared with ControlHubWidget)
  const [liquidGlassConfig, setLiquidGlassConfig] = useState({
    blurAmount: 0.40,
    displacementScale: 64,
    saturation: 135,
    aberrationIntensity: 2,
    elasticity: 0,
    cornerRadius: 28,
    showWallpaper: true,
    showBlobs: true,
  });

  // Guard flag: once persisted settings have been loaded from DB, allow saves
  const [hasLoaded, setHasLoaded] = useState(false);

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
  const [deleteProfileTarget, setDeleteProfileTarget] = useState<string | null>(null);
  const [deleteVarTarget, setDeleteVarTarget] = useState<string | null>(null);

  const [activeTab, setActiveTab] = useState<'theme' | 'visual' | 'env'>('theme');

  const TABS = [
    { id: 'theme' as const, label: t(locale, 'settings.tabTheme') },
    { id: 'visual' as const, label: t(locale, 'settings.tabVisual') },
    { id: 'env' as const, label: t(locale, 'settings.tabEnv') },
  ];

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2200);
  }, []);

  // Load persisted settings on mount
  useEffect(() => {
    async function loadSettings() {
      try {
        const api = window.nativesAPI;
        if (!api) return;
        const [savedTheme, savedLocale] = await Promise.all([
          api.getTheme().catch(() => null),
          api.getLocale().catch(() => null),
        ]);
        if (savedTheme) {
          setThemeState(savedTheme);
          applyTheme(savedTheme);
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

          // Load visual configs so opening Settings doesn't clobber saved values
          const savedVisuals = await db.get(CONFIG_DB_KEY).catch(() => null);
          if (savedVisuals) {
            try {
              const saved = JSON.parse(savedVisuals as string);
              if (saved) {
                setLiquidGlassConfig((prev) => ({
                  ...prev,
                  ...(typeof saved.blurAmount === 'number' && { blurAmount: saved.blurAmount }),
                  ...(typeof saved.displacementScale === 'number' && { displacementScale: saved.displacementScale }),
                  ...(typeof saved.saturation === 'number' && { saturation: saved.saturation }),
                  ...(typeof saved.aberrationIntensity === 'number' && { aberrationIntensity: saved.aberrationIntensity }),
                  ...(typeof saved.elasticity === 'number' && { elasticity: saved.elasticity }),
                  ...(typeof saved.cornerRadius === 'number' && { cornerRadius: saved.cornerRadius }),
                  ...(typeof saved.showWallpaper === 'boolean' && { showWallpaper: saved.showWallpaper }),
                  ...(typeof saved.showBlobs === 'boolean' && { showBlobs: saved.showBlobs }),
                }));
              }
            } catch { /* ignore parse errors */ }
          }
        }
      } catch { /* browser dev mode */ } finally {
        setHasLoaded(true);
      }
    }
    loadSettings();
  }, []);

  // Load environment profiles
  const loadProfiles = useCallback(async () => {
    try {
      const api = window.nativesAPI;
      if (!api?.env) return;
      const list = await api.env.listProfiles();
      setProfiles(list as EnvProfile[]);
      // Auto-select first profile if none selected
      if (!selectedProfile && (list as EnvProfile[]).length > 0) {
        setSelectedProfile((list as EnvProfile[])[0]!.name);
      }
    } catch { /* browser dev mode */ }
  }, [selectedProfile]);

  // Load variables for selected profile
  const loadVariables = useCallback(async (profileName: string) => {
    try {
      const api = window.nativesAPI;
      if (!api?.env) return;
      const vars = await api.env.getVariables(profileName);
      setVariables(
        Object.entries(vars).map(([key, value]) => ({ key, value }))
      );
    } catch { /* browser dev mode */ }
  }, []);

  useEffect(() => {
    loadProfiles();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (selectedProfile) {
      loadVariables(selectedProfile);
    }
  }, [selectedProfile, loadVariables]);

  const THEMES = [
    { id: 'terminal-volt', label: t(locale, 'settings.themeTerminal'), desc: t(locale, 'settings.themeDescTerminal') },
    { id: 'frosted-jasmine', label: t(locale, 'settings.themeJasmine'), desc: t(locale, 'settings.themeDescJasmine') },
  ];

  const LOCALES = [
    { id: 'zh-CN', label: '中文' },
    { id: 'en', label: 'English' },
  ];

  const handleThemeChange = (themeId: string) => {
    setThemeState(themeId);
    applyTheme(themeId);
    try { window.nativesAPI?.setTheme?.(themeId); } catch { /* browser dev mode */ }
  };

  const handleLocaleChange = async (localeId: string) => {
    setLocaleState(localeId as Locale);
    document.documentElement.lang = localeId;
    try {
      await window.nativesAPI?.setLocale?.(localeId);
      // Notify all locale-aware components to refresh
      window.dispatchEvent(new CustomEvent('locale-changed', { detail: localeId }));
    } catch { /* browser dev mode */ }
  };

  // Liquid glass visual config — 500ms debounced save to SQLite
  const CONFIG_DB_KEY = 'settings:controlHubVisuals';
  useEffect(() => {
    if (!hasLoaded) return;

    // Apply styles instantly to this window (compute and set variables directly to bypass Chromium nested repaint bug)
    applyLiquidGlassConfig(liquidGlassConfig);

    // Dispatch custom event for real-time synchronization in the same window (e.g. ShellLayout background)
    window.dispatchEvent(new CustomEvent('visual-config-changed', { detail: liquidGlassConfig }));

    const api = window.nativesAPI;
    if (!api?.db?.set) return;

    const timer = setTimeout(() => {
      const config = {
        blurAmount: liquidGlassConfig.blurAmount,
        displacementScale: liquidGlassConfig.displacementScale,
        saturation: liquidGlassConfig.saturation,
        aberrationIntensity: liquidGlassConfig.aberrationIntensity,
        elasticity: liquidGlassConfig.elasticity,
        cornerRadius: liquidGlassConfig.cornerRadius,
        showWallpaper: liquidGlassConfig.showWallpaper,
        showBlobs: liquidGlassConfig.showBlobs,
      };
      api.db.set(CONFIG_DB_KEY, JSON.stringify(config)).catch(() => {});
    }, 500);
    return () => clearTimeout(timer);
  }, [liquidGlassConfig, hasLoaded]);

  const saveLayoutSetting = (key: string, value: number) => {
    try { window.nativesAPI?.db?.set?.(`settings:${key}`, String(value)); } catch { /* browser dev mode */ }
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
      console.error('[Settings] Create profile failed:', err);
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
      console.error('[Settings] Delete profile failed:', err);
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
      console.error('[Settings] Set variable failed:', err);
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
      console.error('[Settings] Delete variable failed:', err);
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
      console.error('[Settings] Edit variable failed:', err);
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
        <div style={{ display: 'flex', gap: 4, background: 'var(--bg-3)', border: '1px solid var(--vibe-toolbar-border)', borderRadius: 10, padding: 3 }}>
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              style={{
                flex: 1, textAlign: 'center', padding: '7px 4px',
                border: 'none', borderRadius: 8, cursor: 'pointer',
                background: activeTab === tab.id ? 'linear-gradient(135deg, var(--accent) 0%, #ff9856 100%)' : 'transparent',
                color: activeTab === tab.id ? 'var(--accent-ink)' : 'var(--text-dim)',
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
                <div style={{ fontSize: FONT_SIZE.xs, color: theme === th.id ? 'var(--accent-ink)' : 'var(--text-faint)', marginTop: SPACING.xs }}>{th.desc}</div>
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

        {/* About */}
        <div style={sectionCardStyle}>
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.about')}
          </h2>
          <p style={{ fontSize: FONT_SIZE.md, color: 'var(--text-faint)' }}>
            {t(locale, 'settings.aboutVersion')}
          </p>
        </section>
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
              border: '1px dashed var(--vibe-toolbar-border)',
              borderRadius: BORDER_RADIUS.lg,
              color: 'var(--vibe-btn-text)',
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
                        borderRadius: BORDER_RADIUS.md,
                        cursor: 'pointer',
                        background: selectedProfile === p.name ? 'var(--accent-soft)' : 'transparent',
                        border: selectedProfile === p.name ? '1px solid var(--accent)' : '1px solid transparent',
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
                            borderRadius: BORDER_RADIUS.sm,
                            background: 'var(--accent)',
                            color: 'var(--accent-ink)',
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
                            } catch {
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
                  border: '1px solid var(--vibe-toolbar-border)',
                  borderRadius: BORDER_RADIUS.lg,
                  overflow: 'hidden',
                }}>
                  <div style={{
                    padding: `${SPACING.sm}px 10px`,
                    background: 'var(--vibe-toolbar-bg)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    borderBottom: '1px solid var(--vibe-toolbar-border)',
                  }}>
                    <span style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 1 }}>
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
                      <div style={{ padding: `${SPACING.lg}px 10px`, textAlign: 'center', color: 'var(--text-faint)', fontSize: FONT_SIZE.md }}>
                        {t(locale, 'settings.noVariables')}
                      </div>
                    ) : (
                      variables.filter(v => v.value !== '').map((v) => (
                        <div
                          key={v.key}
                          style={{
                            padding: '6px 10px',
                            borderBottom: '1px solid var(--vibe-toolbar-border)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: SPACING.sm,
                          }}
                        >
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{ fontSize: FONT_SIZE.md, fontWeight: 600, color: 'var(--accent)', fontFamily: 'var(--font-mono)' }}>
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
                              <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
                                {maskValue(v.value)}
                              </div>
                            )}
                          </div>
                          <div style={{ display: 'flex', gap: 2, flexShrink: 0 }}>
                            <button
                              className="btn"
                              style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px' }}
                              onClick={() => { setEditingVar(v.key); setEditValue(v.value); }}
                              title="Edit"
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

        {/* Tab 2: 视觉微调 */}
        <div style={{ display: activeTab === 'visual' ? 'flex' : 'none', flexDirection: 'column', gap: 28 }}>
          {/* Liquid Glass Visual Tuning */}
        <section>
          <h2 style={sectionTitleStyle}>
            液态玻璃视觉微调
          </h2>
          <div style={sectionCardStyle}>
            <div style={{ display: 'flex', flexDirection: 'column', gap: SPACING.md }}>
              {/* Blur Amount: 0–1, default 0.40 */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  <span>Blur Amount</span>
                  <span>{liquidGlassConfig.blurAmount.toFixed(2)}</span>
                </div>
                <input type="range" min="0" max="1" step="0.05" value={liquidGlassConfig.blurAmount}
                  onChange={(e) => setLiquidGlassConfig(c => ({ ...c, blurAmount: Number(e.target.value) }))}
                  style={{ width: '100%', accentColor: 'var(--vibe-accent-color)' }} />
              </div>
              {/* Displacement Scale: 0–150, default 64 */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  <span>Displacement Scale</span>
                  <span>{liquidGlassConfig.displacementScale}</span>
                </div>
                <input type="range" min="0" max="150" value={liquidGlassConfig.displacementScale}
                  onChange={(e) => setLiquidGlassConfig(c => ({ ...c, displacementScale: Number(e.target.value) }))}
                  style={{ width: '100%', accentColor: 'var(--vibe-accent-color)' }} />
              </div>
              {/* Saturation: 100–250%, default 135% */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  <span>Saturation</span>
                  <span>{liquidGlassConfig.saturation}%</span>
                </div>
                <input type="range" min="100" max="250" step="5" value={liquidGlassConfig.saturation}
                  onChange={(e) => setLiquidGlassConfig(c => ({ ...c, saturation: Number(e.target.value) }))}
                  style={{ width: '100%', accentColor: 'var(--vibe-accent-color)' }} />
              </div>
              {/* Chromatic Aberration: 0–10, default 2 */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  <span>Chromatic Aberration</span>
                  <span>{liquidGlassConfig.aberrationIntensity}</span>
                </div>
                <input type="range" min="0" max="10" step="1" value={liquidGlassConfig.aberrationIntensity}
                  onChange={(e) => setLiquidGlassConfig(c => ({ ...c, aberrationIntensity: Number(e.target.value) }))}
                  style={{ width: '100%', accentColor: 'var(--vibe-accent-color)' }} />
              </div>
              {/* Elasticity: 0–0.8, default 0 */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  <span>Elasticity</span>
                  <span>{liquidGlassConfig.elasticity.toFixed(2)}</span>
                </div>
                <input type="range" min="0" max="0.8" step="0.05" value={liquidGlassConfig.elasticity}
                  onChange={(e) => setLiquidGlassConfig(c => ({ ...c, elasticity: Number(e.target.value) }))}
                  style={{ width: '100%', accentColor: 'var(--vibe-accent-color)' }} />
              </div>
              {/* Corner Radius: 12–48px, default 28px */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  <span>Corner Radius</span>
                  <span>{liquidGlassConfig.cornerRadius}px</span>
                </div>
                <input type="range" min="12" max="48" step="2" value={liquidGlassConfig.cornerRadius}
                  onChange={(e) => setLiquidGlassConfig(c => ({ ...c, cornerRadius: Number(e.target.value) }))}
                  style={{ width: '100%', accentColor: 'var(--vibe-accent-color)' }} />
              </div>
              {/* Toggles */}
              <div style={{ display: 'flex', gap: SPACING.lg, marginTop: SPACING.xs }}>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', cursor: 'pointer' }}>
                  <input type="checkbox" checked={liquidGlassConfig.showWallpaper}
                    onChange={(e) => setLiquidGlassConfig(c => ({ ...c, showWallpaper: e.target.checked }))} />
                  Mockup Wallpaper
                </label>
                <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', cursor: 'pointer' }}>
                  <input type="checkbox" checked={liquidGlassConfig.showBlobs}
                    onChange={(e) => setLiquidGlassConfig(c => ({ ...c, showBlobs: e.target.checked }))} />
                  Bubbles
                </label>
              </div>
            </div>
            {/* Live preview mini card */}
            <div style={{
              marginTop: SPACING.md, padding: 12, borderRadius: 12,
              background: 'linear-gradient(135deg, rgba(255,255,255,0.08) 0%, rgba(255,255,255,0.02) 100%)',
              border: '1px solid var(--border)',
              textAlign: 'center', fontSize: FONT_SIZE.sm, color: 'var(--text-dim)',
            }}>
              <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, marginBottom: 6, textTransform: 'uppercase', letterSpacing: 1 }}>
                LiquidGlass Preview
              </div>
              <div style={{
                width: '100%', height: 40, borderRadius: liquidGlassConfig.cornerRadius,
                background: 'linear-gradient(135deg, rgba(255,154,86,0.2) 0%, rgba(240,91,63,0.15) 100%)',
                backdropFilter: `blur(${liquidGlassConfig.blurAmount * 40}px) saturate(${liquidGlassConfig.saturation}%)`,
                WebkitBackdropFilter: `blur(${liquidGlassConfig.blurAmount * 40}px) saturate(${liquidGlassConfig.saturation}%)`,
                border: '0.5px solid rgba(255,255,255,0.15)',
              }} />
            </div>
          </div>
        </section>
 
        {/* Widget Launcher */}
        <section>
          <h2 style={sectionTitleStyle}>
            控制中心挂件 (Natives Control Hub)
          </h2>
          <div style={sectionCardStyle}>
            <p style={{ fontSize: FONT_SIZE.md, color: 'var(--text-faint)', margin: 0 }}>
              唤起桌面悬浮挂件窗口，提供液态玻璃视觉控制器、系统监控和快捷开关。
            </p>
            <div>
              <button
                className="btn btn-primary"
                onClick={() => {
                  try { window.nativesAPI?.openWidgetWindow?.(); } catch {}
                }}
                style={{ fontSize: FONT_SIZE.md, padding: '6px 14px' }}
              >
                唤起桌面悬浮挂件
              </button>
            </div>
          </div>
        </section>
        </div>
        {/* ⇡ Tab 2 end ⇡ */}
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

      {/* Toast */}
      {toast && (
        <div style={{
          position: 'fixed',
          bottom: SPACING.xl,
          left: '50%',
          transform: 'translateX(-50%)',
          background: 'var(--vibe-btn-bg)',
          border: '1px solid var(--vibe-btn-border)',
          padding: '10px 18px',
          borderRadius: BORDER_RADIUS.xl,
          fontSize: FONT_SIZE.lg,
          color: 'var(--text)',
          zIndex: 200,
          animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
          boxShadow: 'var(--vibe-sidebar-shadow)',
          backdropFilter: 'blur(12px)',
          WebkitBackdropFilter: 'blur(12px)',
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
  color: 'var(--vibe-btn-text)',
  marginBottom: SPACING.sm,
  textTransform: 'uppercase',
  letterSpacing: 1,
};

const inputStyle: React.CSSProperties = {
  width: 80,
  padding: `${SPACING.xs}px ${SPACING.sm}px`,
  background: 'var(--vibe-content-bg)',
  border: '1px solid var(--vibe-btn-border)',
  borderRadius: BORDER_RADIUS.sm,
  color: 'var(--text)',
  fontSize: FONT_SIZE.md,
};

const sectionCardStyle: React.CSSProperties = {
  background: 'var(--bg-2)',
  border: '1px solid var(--border)',
  borderRadius: BORDER_RADIUS.md,
  padding: `${SPACING.md}px ${SPACING.lg}px`,
  display: 'flex',
  flexDirection: 'column',
  gap: SPACING.md,
};
