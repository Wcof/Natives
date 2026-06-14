'use client';

import { useState, useEffect } from 'react';
import { applyTheme } from '@/lib/theme-engine';

export default function SettingsPage() {
  const [theme, setThemeState] = useState('terminal-volt');
  const [locale, setLocaleState] = useState('zh-CN');

  // Load persisted settings on mount
  useEffect(() => {
    async function loadSettings() {
      try {
        const api = (window as unknown as Record<string, unknown>).nativesAPI as Record<string, unknown> | undefined;
        if (api) {
          // Load theme
          const getTheme = (api as Record<string, unknown>).getTheme as (() => string) | undefined;
          if (getTheme) {
            const savedTheme = await getTheme();
            if (savedTheme) setThemeState(savedTheme);
          }
          // Load locale
          // In a real implementation, this would call an IPC to read the saved locale
        }
      } catch { /* browser dev mode */ }
    }
    loadSettings();
  }, []);

  const THEMES = [
    { id: 'terminal-volt', label: 'Terminal Volt', desc: 'Dark, terminal-inspired' },
    { id: 'warm-archive', label: 'Warm Archive', desc: 'Warm, paper-like' },
    { id: 'editorial-index', label: 'Editorial Index', desc: 'Editorial, high contrast' },
  ];

  const LOCALES = [
    { id: 'zh-CN', label: '中文' },
    { id: 'en', label: 'English' },
  ];

  const handleThemeChange = (themeId: string) => {
    setThemeState(themeId);
    applyTheme(themeId);
    // Persist to backend
    try {
      const api = (window as unknown as Record<string, unknown>).nativesAPI as Record<string, unknown> | undefined;
      if (api) {
        const setTheme = (api as Record<string, unknown>).setTheme as ((t: string) => void) | undefined;
        setTheme?.(themeId);
      }
    } catch { /* browser dev mode */ }
  };

  const handleLocaleChange = (localeId: string) => {
    setLocaleState(localeId);
    // Persist to backend
    try {
      const api = (window as unknown as Record<string, unknown>).nativesAPI as Record<string, unknown> | undefined;
      if (api) {
        // Would call IPC to persist locale
      }
    } catch { /* browser dev mode */ }
  };

  return (
    <div style={{ height: '100%', overflow: 'auto' }}>
      <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border,#262920)' }}>
        <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0 }}>
          Settings
        </h1>
      </div>

      <div style={{ padding: '16px 20px', display: 'flex', flexDirection: 'column', gap: 24 }}>
        {/* Theme */}
        <section>
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            Theme
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
            Language
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

        {/* Environment Profiles */}
        <section>
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            Environment Profiles
          </h2>
          <p style={{ fontSize: 12, color: 'var(--text-faint)', marginBottom: 8 }}>
            Manage API keys and environment variables for CLI tools
          </p>
          <button className="btn" style={{ width: '100%' }}>
            + Add Profile
          </button>
        </section>

        {/* About */}
        <section>
          <h2 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-dim,#9b9d8c)', marginBottom: 8, textTransform: 'uppercase', letterSpacing: 1 }}>
            About
          </h2>
          <p style={{ fontSize: 12, color: 'var(--text-faint)' }}>
            Natives v0.1.0 — AI Steam Base
          </p>
        </section>
      </div>
    </div>
  );
}
