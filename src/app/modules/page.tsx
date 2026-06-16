'use client';

import { useState, useEffect } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { Power, Trash2, RefreshCw } from 'lucide-react';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { t, type Locale } from '@/i18n';
import { EmptyState } from '@/components/ui/EmptyState';

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
      <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border,#262920)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <h1 style={{ fontSize: 17, fontWeight: 600, color: 'var(--text,#f2f2ea)', margin: 0 }}>{t(locale, 'modules.title')}</h1>
        <button
          onClick={handleScan}
          style={{ background: 'var(--bg-3,#1c1e17)', border: '1px solid var(--border,#262920)', color: 'var(--text,#f2f2ea)', padding: '6px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12, display: 'flex', alignItems: 'center', gap: 6 }}
          aria-label={t(locale, 'modules.ariaScan')}
        >
          <RefreshCw size={14} />
          {t(locale, 'modules.scan')}
        </button>
      </div>

      {loading ? (
        <div style={{ padding: '60px 20px', textAlign: 'center', color: 'var(--text-faint,#62655a)', fontSize: 13 }}>
          {t(locale, 'common.loading')}
        </div>
      ) : (modules ?? []).length === 0 ? (
        <EmptyState title={t(locale, 'modules.emptyState')} />
      ) : (
        <div aria-live="polite">
          {(modules ?? []).map((mod) => (
            <div key={mod.id} style={{
              display: 'flex', alignItems: 'center', gap: 12,
              padding: '12px 20px', borderBottom: '1px solid var(--border,#262920)',
            }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text,#f2f2ea)' }}>{mod.name}</div>
                <div style={{ fontSize: 11, color: 'var(--text-faint,#62655a)' }}>{mod.id} v{mod.version}</div>
                {mod.description && <div style={{ fontSize: 12, color: 'var(--text-dim,#9b9d8c)', marginTop: 4 }}>{mod.description}</div>}
              </div>
              <span style={{
                fontSize: 11, padding: '2px 8px', borderRadius: 4,
                background: mod.enabled ? 'var(--accent-soft,#cdf24b1f)' : 'var(--bg-3,#1c1e17)',
                color: mod.enabled ? 'var(--accent,#cdf24b)' : 'var(--text-faint,#62655a)',
              }}>
                {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
              </span>
              <button
                onClick={() => handleToggle(mod)}
                style={{ background: 'none', border: '1px solid var(--border,#262920)', color: 'var(--text-dim,#9b9d8c)', padding: '4px 8px', borderRadius: 4, cursor: 'pointer' }}
                aria-label={mod.enabled ? t(locale, 'modules.ariaDisable').replace('{name}', mod.name) : t(locale, 'modules.ariaEnable').replace('{name}', mod.name)}
              >
                <Power size={14} />
              </button>
              <button
                onClick={() => handleUninstall(mod)}
                style={{ background: 'none', border: '1px solid var(--border,#262920)', color: 'var(--danger)', padding: '4px 8px', borderRadius: 4, cursor: 'pointer' }}
                aria-label={t(locale, 'modules.ariaUninstall').replace('{name}', mod.name)}
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
