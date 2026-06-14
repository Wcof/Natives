'use client';

import { useState, useEffect } from 'react';
import { applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';

export default function SettingsPage() {
  const [theme, setThemeState] = useState('terminal-volt');
  const [locale, setLocaleState] = useState<Locale>('zh');
  const [sidebarWidth, setSidebarWidth] = useState(248);
  const [panelWidth, setPanelWidth] = useState(320);
  const [terminalHeight, setTerminalHeight] = useState(280);

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
        if (savedLocale) setLocaleState(savedLocale === 'en' ? 'en' : 'zh');

        // Load layout settings
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

  const THEMES = [
    { id: 'terminal-volt', label: 'Terminal Volt', desc: 'Dark, terminal-inspired' },
    { id: 'warm-archive', label: 'Warm Archive', desc: 'Warm, paper-like' },
  ];

  const LOCALES = [
    { id: 'zh-CN', label: '中文' },
    { id: 'en', label: 'English' },
  ];

  const handleThemeChange = (themeId: string) => {
    setThemeState(themeId);
    applyTheme(themeId);
    try {
      window.nativesAPI?.setTheme?.(themeId);
    } catch { /* browser dev mode */ }
  };

  const handleLocaleChange = (localeId: string) => {
    setLocaleState(localeId as Locale);
    document.documentElement.lang = localeId;
    try {
      window.nativesAPI?.setLocale?.(localeId);
    } catch { /* browser dev mode */ }
  };

  const saveLayoutSetting = (key: string, value: number) => {
    try {
      window.nativesAPI?.db?.set?.(`settings:${key}`, String(value));
    } catch { /* browser dev mode */ }
  };

  return (
    <div style={{ height: '100%', overflow: 'auto' }}>
      <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border,#262920)' }}>
        <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0 }}>
          {t(locale, 'settings.title')}
        </h1>
      </div>

      <div style={{ padding: '16px 20px', display: 'flex', flexDirection: 'column', gap: 24 }}>
        {/* Theme */}
        <section>
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            {t(locale, 'settings.theme')}
          </h2>
          <div style={{ display: 'flex', gap: 8 }}>
            {THEMES.map((t) => (
              <button
                key={t.id}
                className={`btn ${theme === t.id ? 'btn-primary' : ''}`}
                onClick={() => handleThemeChange(t.id)}
                style={{ flex: 1, textAlign: 'center', padding: '10px 8px' }}
              >
                <div style={{ fontWeight: 600, fontSize: 12 }}>{t.label}</div>
                <div style={{ fontSize: 10, color: 'var(--text-faint)', marginTop: 4 }}>{t.desc}</div>
              </button>
            ))}
          </div>
        </section>

        {/* Language */}
        <section>
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
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
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            Layout
          </h2>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {[
              { label: 'Sidebar Width', value: sidebarWidth, key: 'sidebar_width', set: setSidebarWidth, min: 190, max: 420 },
              { label: 'Panel Width', value: panelWidth, key: 'panel_width', set: setPanelWidth, min: 200, max: 600 },
              { label: 'Terminal Height', value: terminalHeight, key: 'terminal_height', set: setTerminalHeight, min: 100, max: 600 },
            ].map((item) => (
              <div key={item.key} style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <label style={{ fontSize: 12, color: 'var(--text)' }}>{item.label}</label>
                <input
                  type="number"
                  value={item.value}
                  min={item.min}
                  max={item.max}
                  style={{ width: 80, padding: '4px 8px', background: 'var(--bg-surface,#1a1c14)', border: '1px solid var(--border,#262920)', borderRadius: 4, color: 'var(--text)', fontSize: 12 }}
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
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            {t(locale, 'settings.environment')}
          </h2>
          <p style={{ fontSize: 12, color: 'var(--text-faint)', marginBottom: 8 }}>
            Manage API keys and environment variables for CLI tools
          </p>
          <button className="btn" style={{ width: '100%' }}>
            + {t(locale, 'settings.addProfile')}
          </button>
        </section>

        {/* About */}
        <section>
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            {t(locale, 'settings.about')}
          </h2>
          <p style={{ fontSize: 12, color: 'var(--text-faint)' }}>
            Natives v0.1.0 — AI Steam Base
          </p>
        </section>
      </div>
    </div>
  );
}
