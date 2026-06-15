'use client';

import { useState, useEffect, useCallback } from 'react';
import { t, type Locale } from '@/i18n';

interface CrashRecord {
  moduleId: string;
  title: string;
  time: number;
  recovered: boolean;
}

/**
 * Crash Monitor panel (TASK-021).
 * Shows crashed modules with reload/recovery controls.
 * Connected to ShellLayout's crashedModules state via custom event.
 */
export default function CrashMonitor() {
  const [crashes, setCrashes] = useState<CrashRecord[]>([]);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // Listen for crash events
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as { moduleId: string; title?: string } | undefined;
      if (detail?.moduleId) {
        setCrashes((prev) => {
          // Avoid duplicates
          if (prev.some((c) => c.moduleId === detail.moduleId && !c.recovered)) return prev;
          return [{ moduleId: detail.moduleId, title: detail.title || `Module ${detail.moduleId} crashed`, time: Date.now(), recovered: false }, ...prev].slice(0, 20);
        });
      }
    };
    window.addEventListener('module-crashed', handler);
    return () => window.removeEventListener('module-crashed', handler);
  }, []);

  const handleReload = useCallback((moduleId: string) => {
    window.dispatchEvent(new CustomEvent('reload-module', { detail: moduleId }));
    setCrashes((prev) => prev.map((c) => c.moduleId === moduleId ? { ...c, recovered: true } : c));
  }, []);

  const handleReloadAll = useCallback(() => {
    for (const crash of crashes) {
      if (!crash.recovered) {
        window.dispatchEvent(new CustomEvent('reload-module', { detail: crash.moduleId }));
      }
    }
    setCrashes((prev) => prev.map((c) => ({ ...c, recovered: true })));
  }, [crashes]);

  if (crashes.length === 0) return null;

  const activeCrashes = crashes.filter((c) => !c.recovered);

  return (
    <div style={{
      position: 'fixed', bottom: 60, right: 16, zIndex: 100,
      width: 320, maxHeight: 240, overflow: 'auto',
      background: 'var(--bg-2,#131410)',
      border: '1px solid #f24b4b',
      borderRadius: 8,
      boxShadow: '0 4px 20px rgba(0,0,0,0.3)',
    }}>
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '8px 10px', borderBottom: '1px solid var(--border,#262920)',
        background: '#f24b4b15',
      }}>
        <span style={{ fontSize: 11, fontWeight: 600, color: '#f24b4b' }}>
          ⚠️ {activeCrashes.length} {activeCrashes.length !== 1 ? t(locale, 'errors.crashes') : t(locale, 'errors.crash')}
        </span>
        {activeCrashes.length > 1 && (
          <button onClick={handleReloadAll} style={{ fontSize: 9, padding: '2px 6px', borderRadius: 3, border: '1px solid var(--border,#262920)', background: 'transparent', color: 'var(--text-faint)', cursor: 'pointer' }}>
            {t(locale, 'errors.reloadAll')}
          </button>
        )}
      </div>
      {crashes.map((crash) => (
        <div key={crash.moduleId + crash.time} style={{
          display: 'flex', alignItems: 'center', gap: 6,
          padding: '6px 10px', fontSize: 11,
          borderBottom: '1px solid var(--border,#262920)',
          opacity: crash.recovered ? 0.5 : 1,
        }}>
          <span style={{ flex: 1, color: crash.recovered ? 'var(--text-dim)' : 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {crash.title}
          </span>
          {crash.recovered ? (
            <span style={{ fontSize: 9, color: 'var(--accent,#cdf24b)' }}>✓ {t(locale, 'errors.recovered')}</span>
          ) : (
            <button onClick={() => handleReload(crash.moduleId)} style={{ fontSize: 9, padding: '2px 5px', borderRadius: 3, border: '1px solid var(--accent,#cdf24b)', background: 'transparent', color: 'var(--accent,#cdf24b)', cursor: 'pointer' }}>
              ↻ {t(locale, 'errors.reload')}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
