'use client';

import { useState, useEffect, useCallback } from 'react';
import { applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { X, Check, Star, Edit2 } from 'lucide-react';

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
  const [theme, setThemeState] = useState('editorial');
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
  const [deleteProfileTarget, setDeleteProfileTarget] = useState<string | null>(null);
  const [deleteVarTarget, setDeleteVarTarget] = useState<string | null>(null);

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
        }
      } catch { /* browser dev mode */ }
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
    { id: 'warm-archive', label: 'Warm Archive', desc: t(locale, 'settings.themeDescWarm') },
    { id: 'editorial', label: 'Editorial', desc: t(locale, 'settings.themeDescEditorial') },
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
      <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border,#262920)' }}>
        <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0, fontFamily: 'var(--font-display)' }}>
          {t(locale, 'settings.title')}
        </h1>
      </div>

      <div style={{ padding: '16px 20px', display: 'flex', flexDirection: 'column', gap: 28 }}>
        {/* Theme */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.theme')}
          </h2>
          <div style={{ display: 'flex', gap: 8 }}>
            {THEMES.map((th) => (
              <button
                key={th.id}
                className={`btn ${theme === th.id ? 'btn-primary' : ''}`}
                onClick={() => handleThemeChange(th.id)}
                style={{ flex: 1, textAlign: 'center', padding: '10px 8px' }}
              >
                <div style={{ fontWeight: 600, fontSize: 12 }}>{th.label}</div>
                <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 4 }}>{th.desc}</div>
              </button>
            ))}
          </div>
        </section>

        {/* Language */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.language')}
          </h2>
          <div style={{ display: 'flex', gap: 8 }}>
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
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.layout')}
          </h2>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {[
              { label: t(locale, 'settings.sidebarWidth'), value: sidebarWidth, key: 'sidebar_width', set: setSidebarWidth, min: 190, max: 420 },
              { label: t(locale, 'settings.panelWidth'), value: panelWidth, key: 'panel_width', set: setPanelWidth, min: 200, max: 600 },
              { label: t(locale, 'settings.terminalHeight'), value: terminalHeight, key: 'terminal_height', set: setTerminalHeight, min: 100, max: 600 },
            ].map((item) => (
              <div key={item.key} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <label style={{ fontSize: 12, color: 'var(--text)' }}>{item.label}</label>
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

        {/* Environment Profiles */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.environment')}
          </h2>

          {profiles.length === 0 && !showNewProfile ? (
            <div style={{
              padding: '24px 16px',
              textAlign: 'center',
              border: '1px dashed var(--border,#262920)',
              borderRadius: 8,
              color: 'var(--text-faint,#62655a)',
            }}>
              <div style={{ fontSize: 13, marginBottom: 4 }}>{t(locale, 'settings.noProfiles')}</div>
              <div style={{ fontSize: 11, marginBottom: 12 }}>{t(locale, 'settings.noProfilesDesc')}</div>
              <button className="btn btn-primary" onClick={() => setShowNewProfile(true)}>
                + {t(locale, 'settings.addProfile')}
              </button>
            </div>
          ) : (
            <>
              {/* Profile list */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4, marginBottom: 12 }}>
                {profiles.map((p) => (
                  <div
                    key={p.id}
                    onClick={() => setSelectedProfile(p.name)}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      padding: '8px 10px',
                      borderRadius: 6,
                      cursor: 'pointer',
                      background: selectedProfile === p.name ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
                      border: selectedProfile === p.name ? '1px solid var(--accent,#cdf24b)' : '1px solid transparent',
                      transition: 'all 0.12s',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span style={{ fontSize: 13, color: 'var(--text)', fontWeight: selectedProfile === p.name ? 600 : 400 }}>
                        {p.name}
                      </span>
                      {p.is_default === 1 && (
                        <span style={{
                          fontSize: 9,
                          padding: '1px 5px',
                          borderRadius: 3,
                          background: 'var(--accent,#cdf24b)',
                          color: 'var(--accent-ink,#0b0c0a)',
                          fontWeight: 600,
                          textTransform: 'uppercase',
                          letterSpacing: 0.5,
                        }}>
                          {t(locale, 'settings.defaultProfile')}
                        </span>
                      )}
                    </div>
                    <div style={{ display: 'flex', gap: 4 }}>
                      <button
                        className="btn"
                        style={{ fontSize: 10, padding: '2px 6px' }}
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
                        style={{ fontSize: 10, padding: '2px 6px', color: 'var(--danger)' }}
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

              {/* Add profile inline */}
              {showNewProfile ? (
                <div style={{ display: 'flex', gap: 6, marginBottom: 12 }}>
                  <input
                    type="text"
                    value={newProfileName}
                    onChange={(e) => setNewProfileName(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleCreateProfile()}
                    placeholder={t(locale, 'settings.profileNamePlaceholder')}
                    style={{ ...inputStyle, flex: 1 }}
                    autoFocus
                  />
                  <button className="btn btn-primary" onClick={handleCreateProfile} style={{ fontSize: 12 }}>
                    {t(locale, 'common.confirm')}
                  </button>
                  <button className="btn" onClick={() => { setShowNewProfile(false); setNewProfileName(''); }} style={{ fontSize: 12 }}>
                    {t(locale, 'common.cancel')}
                  </button>
                </div>
              ) : (
                <button className="btn" style={{ width: '100%', marginBottom: 12 }} onClick={() => setShowNewProfile(true)}>
                  + {t(locale, 'settings.addProfile')}
                </button>
              )}

              {/* Variables for selected profile */}
              {selectedProfile && (
                <div style={{
                  border: '1px solid var(--border,#262920)',
                  borderRadius: 8,
                  overflow: 'hidden',
                }}>
                  <div style={{
                    padding: '8px 10px',
                    background: 'var(--bg-2,#131410)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    borderBottom: '1px solid var(--border,#262920)',
                  }}>
                    <span style={{ fontSize: 11, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 1 }}>
                      {t(locale, 'settings.variables')}
                    </span>
                    <button
                      className="btn"
                      style={{ fontSize: 10, padding: '2px 8px' }}
                      onClick={() => setShowNewVar(true)}
                    >
                      + {t(locale, 'settings.addVariable')}
                    </button>
                  </div>

                  {/* Variable list */}
                  <div style={{ maxHeight: 240, overflow: 'auto' }}>
                    {variables.filter(v => v.value !== '').length === 0 && !showNewVar ? (
                      <div style={{ padding: '16px 10px', textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
                        {t(locale, 'settings.noVariables')}
                      </div>
                    ) : (
                      variables.filter(v => v.value !== '').map((v) => (
                        <div
                          key={v.key}
                          style={{
                            padding: '6px 10px',
                            borderBottom: '1px solid var(--border,#262920)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                            gap: 8,
                          }}
                        >
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--accent,#cdf24b)', fontFamily: 'var(--font-mono)' }}>
                              {v.key}
                            </div>
                            {editingVar === v.key ? (
                              <div style={{ display: 'flex', gap: 4, marginTop: 4 }}>
                                <input
                                  type="password"
                                  value={editValue}
                                  onChange={(e) => setEditValue(e.target.value)}
                                  onKeyDown={(e) => e.key === 'Enter' && handleEditVariable(v.key)}
                                  style={{ ...inputStyle, flex: 1, fontSize: 11, padding: '4px 6px' }}
                                  autoFocus
                                />
                                <button className="btn btn-primary" style={{ fontSize: 10, padding: '2px 6px' }} onClick={() => handleEditVariable(v.key)}>
                                  <Check size={12} />
                                </button>
                                <button className="btn" style={{ fontSize: 10, padding: '2px 6px' }} onClick={() => { setEditingVar(null); setEditValue(''); }}>
                                  <X size={12} />
                                </button>
                              </div>
                            ) : (
                              <div style={{ fontSize: 11, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
                                {maskValue(v.value)}
                              </div>
                            )}
                          </div>
                          <div style={{ display: 'flex', gap: 2, flexShrink: 0 }}>
                            <button
                              className="btn"
                              style={{ fontSize: 10, padding: '2px 6px' }}
                              onClick={() => { setEditingVar(v.key); setEditValue(v.value); }}
                              title="Edit"
                            >
                              <Edit2 size={12} />
                            </button>
                            <button
                              className="btn"
                              style={{ fontSize: 10, padding: '2px 6px', color: 'var(--danger)' }}
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
                      <div style={{ padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 6 }}>
                        <input
                          type="text"
                          value={newVarKey}
                          onChange={(e) => setNewVarKey(e.target.value)}
                          placeholder={t(locale, 'settings.variableKeyPlaceholder')}
                          style={{ ...inputStyle, fontSize: 12, padding: '6px 8px' }}
                          autoFocus
                        />
                        <input
                          type="password"
                          value={newVarValue}
                          onChange={(e) => setNewVarValue(e.target.value)}
                          onKeyDown={(e) => e.key === 'Enter' && handleAddVariable()}
                          placeholder={t(locale, 'settings.variableValuePlaceholder')}
                          style={{ ...inputStyle, fontSize: 12, padding: '6px 8px' }}
                        />
                        <div style={{ display: 'flex', gap: 6 }}>
                          <button className="btn btn-primary" onClick={handleAddVariable} style={{ fontSize: 11, flex: 1 }}>
                            {t(locale, 'common.save')}
                          </button>
                          <button className="btn" onClick={() => { setShowNewVar(false); setNewVarKey(''); setNewVarValue(''); }} style={{ fontSize: 11 }}>
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

        {/* About */}
        <section>
          <h2 style={sectionTitleStyle}>
            {t(locale, 'settings.about')}
          </h2>
          <p style={{ fontSize: 12, color: 'var(--text-faint)' }}>
            {t(locale, 'settings.aboutVersion')}
          </p>
        </section>
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
          bottom: 24,
          left: '50%',
          transform: 'translateX(-50%)',
          background: 'var(--bg-3,#1c1e17)',
          border: '1px solid var(--border,#262920)',
          padding: '10px 18px',
          borderRadius: 10,
          fontSize: 13,
          color: 'var(--text)',
          zIndex: 200,
          animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
        }}>
          {toast}
        </div>
      )}
    </div>
  );
}

const sectionTitleStyle: React.CSSProperties = {
  fontSize: 13,
  fontWeight: 600,
  color: 'var(--text-dim,#9b9d8c)',
  marginBottom: 8,
  textTransform: 'uppercase',
  letterSpacing: 1,
};

const inputStyle: React.CSSProperties = {
  width: 80,
  padding: '4px 8px',
  background: 'var(--bg,#0b0c0a)',
  border: '1px solid var(--border,#262920)',
  borderRadius: 4,
  color: 'var(--text)',
  fontSize: 12,
};
