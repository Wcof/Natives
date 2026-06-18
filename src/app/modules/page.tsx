'use client';

import { useState, useEffect } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { Power, Trash2, RefreshCw } from 'lucide-react';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { t, type Locale } from '@/i18n';
import { EmptyState } from '@/components/ui/EmptyState';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface ModuleInfo {
  id: string;
  name: string;
  version: string;
  enabled: number;
  state: string;
  description?: string;
  author?: string;
}

export default function ModulesPage() {
  const { data: modules, loading, error, reload: loadModules } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (api?.module?.list) {
      const result = await api.module.list();
      if (Array.isArray(result)) return result as ModuleInfo[];
    }
    return [];
  }, []);
  const [uninstallTarget, setUninstallTarget] = useState<ModuleInfo | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  async function handleToggle(mod: ModuleInfo) {
    try {
      const api = window.nativesAPI;
      if (mod.enabled) {
        await api?.module?.disable?.(mod.id);
      } else {
        await api?.module?.enable?.(mod.id);
      }
      await loadModules();
    } catch (err) {
      console.error('[Modules] Toggle failed:', err);
    }
  }

  async function doUninstall() {
    if (!uninstallTarget) return;
    try {
      const api = window.nativesAPI;
      await api?.module?.uninstall?.(uninstallTarget.id);
      await loadModules();
    } catch (err) {
      console.error('[Modules] Uninstall failed:', err);
    } finally {
      setUninstallTarget(null);
    }
  }

  function handleUninstall(mod: ModuleInfo) {
    setUninstallTarget(mod);
  }

  async function handleScan() {
    try {
      const api = window.nativesAPI;
      await api?.module?.scan?.();
      await loadModules();
    } catch (err) {
      console.error('[Modules] Scan failed:', err);
    }
  }

  return (
    <div style={{ height: '100%', overflow: 'auto' }} role="region" aria-label="Module manager">
      {/* Minimal action bar — no title (shown in top Header) */}
      <div style={{ padding: `${SPACING.md}px ${SPACING.xl}px`, display: 'flex', alignItems: 'center', justifyContent: 'flex-end' }}>
        <button
          onClick={handleScan}
          style={{ background: 'var(--vibe-btn-bg)', border: '1px solid var(--vibe-btn-border)', color: 'var(--vibe-brand-text)', padding: `${SPACING.xs}px ${SPACING.md}px`, borderRadius: BORDER_RADIUS.md, cursor: 'pointer', fontSize: FONT_SIZE.md, display: 'flex', alignItems: 'center', gap: SPACING.xs }}
          aria-label={t(locale, 'modules.ariaScan')}
        >
          <RefreshCw size={14} />
          {t(locale, 'modules.scan')}
        </button>
      </div>

      {loading ? (
        <div style={{ padding: `${SPACING.xxl}px ${SPACING.xl}px`, textAlign: 'center', color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.lg }}>
          {t(locale, 'common.loading')}
        </div>
      ) : (modules ?? []).length === 0 ? (
        <EmptyState title={t(locale, 'modules.emptyState')} />
      ) : (
        <div aria-live="polite">
          {(modules ?? []).map((mod) => (
            <div key={mod.id} style={{
              display: 'flex', alignItems: 'center', gap: SPACING.md,
              padding: `${SPACING.md}px ${SPACING.xl}px`, borderBottom: '1px solid var(--vibe-btn-border)',
            }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--vibe-brand-text)' }}>{mod.name}</div>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)' }}>{mod.id} v{mod.version}</div>
                {mod.description && <div style={{ fontSize: FONT_SIZE.md, color: 'var(--vibe-btn-text)', marginTop: SPACING.xs }}>{mod.description}</div>}
              </div>
              <span style={{
                fontSize: FONT_SIZE.sm, padding: '2px 8px', borderRadius: BORDER_RADIUS.sm,
                background: mod.enabled ? 'var(--accent-soft)' : 'var(--vibe-btn-bg)',
                color: mod.enabled ? 'var(--accent)' : 'var(--vibe-btn-text)',
              }}>
                {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
              </span>
              <button
                onClick={() => handleToggle(mod)}
                style={{ background: 'none', border: '1px solid var(--vibe-btn-border)', color: 'var(--vibe-btn-text)', padding: '4px 8px', borderRadius: BORDER_RADIUS.sm, cursor: 'pointer' }}
                aria-label={mod.enabled ? t(locale, 'modules.ariaDisable').replace('{name}', mod.name) : t(locale, 'modules.ariaEnable').replace('{name}', mod.name)}
                title={mod.enabled ? t(locale, 'modules.ariaDisable').replace('{name}', mod.name) : t(locale, 'modules.ariaEnable').replace('{name}', mod.name)}
              >
                <Power size={14} />
              </button>
              <button
                onClick={() => handleUninstall(mod)}
                style={{ background: 'none', border: '1px solid var(--vibe-btn-border)', color: 'var(--danger)', padding: '4px 8px', borderRadius: BORDER_RADIUS.sm, cursor: 'pointer' }}
                aria-label={t(locale, 'modules.ariaUninstall').replace('{name}', mod.name)}
                title={t(locale, 'modules.ariaUninstall').replace('{name}', mod.name)}
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
