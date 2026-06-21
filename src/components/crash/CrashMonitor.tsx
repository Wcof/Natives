'use client';

import { useState, useEffect, useCallback } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS, SHADOW } from '@/lib/design-tokens';
import { AlertTriangle, RefreshCw } from 'lucide-react';
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
      background: 'var(--vibe-toolbar-bg)',
      border: '1px solid var(--danger)',
      borderRadius: BORDER_RADIUS.lg,
      boxShadow: SHADOW.elevated,
    }}>
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '8px 10px', borderBottom: '1px solid var(--vibe-btn-border)',
        background: 'var(--danger-soft)',
      }}>
        <span style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--danger)', display: 'inline-flex', alignItems: 'center', gap: SPACING.xs }}>
          <AlertTriangle size={12} />
          {activeCrashes.length} {activeCrashes.length !== 1 ? t(locale, 'errors.crashes') : t(locale, 'errors.crash')}
        </span>
        {activeCrashes.length > 1 && (
          <button onClick={handleReloadAll} style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm, border: '1px solid var(--vibe-btn-border)', background: 'transparent', color: 'var(--text-faint)', cursor: 'pointer' }}>
            {t(locale, 'errors.reloadAll')}
          </button>
        )}
      </div>
      {crashes.map((crash) => (
        <div key={crash.moduleId + crash.time} style={{
          display: 'flex', alignItems: 'center', gap: SPACING.xs,
          padding: '6px 10px', fontSize: FONT_SIZE.sm,
          borderBottom: '1px solid var(--vibe-btn-border)',
          opacity: crash.recovered ? 0.5 : 1,
        }}>
          <span style={{ flex: 1, color: crash.recovered ? 'var(--text-dim)' : 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {crash.title}
          </span>
          {crash.recovered ? (
            <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--accent)' }}>✓ {t(locale, 'errors.recovered')}</span>
          ) : (
            <button onClick={() => handleReload(crash.moduleId)} style={{ fontSize: FONT_SIZE.xs, padding: '2px 5px', borderRadius: BORDER_RADIUS.sm, border: '1px solid var(--accent)', background: 'transparent', color: 'var(--accent)', cursor: 'pointer' }}>
              <RefreshCw size={9} /> {t(locale, 'errors.reload')}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
