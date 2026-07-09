'use client';

import { useState, useEffect } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { motion, useReducedMotion } from 'framer-motion';
import { Power, Trash2, RefreshCw } from 'lucide-react';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { t, type Locale } from '@/i18n';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';

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
  const prefersReducedMotion = useReducedMotion();
  const { toast } = useToast();
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
      toast(classifyError(err).userMessage, 'error');
    }
  }

  async function doUninstall() {
    if (!uninstallTarget) return;
    try {
      const api = window.nativesAPI;
      await api?.module?.uninstall?.(uninstallTarget.id);
      await loadModules();
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
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
      toast(classifyError(err).userMessage, 'error');
    }
  }

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 60, damping: 16, mass: 1 }}
      style={{ height: '100%', overflow: 'auto' }}
      role="region"
      aria-label={t(locale, 'modules.ariaRegion')}
    >
      {/* Minimal action bar */}
      <div style={{ padding: `${SPACING.md}px ${SPACING.xl}px`, display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)' }}>
          {t(locale, 'modules.title') || '模块管理'}
        </div>
        <button
          onClick={handleScan}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: SPACING.xs,
            background: 'var(--surface)', border: '1px solid var(--border)',
            color: 'var(--text)', padding: `${SPACING.xs}px ${SPACING.md}px`,
            borderRadius: BORDER_RADIUS.md, cursor: 'pointer', fontSize: FONT_SIZE.sm,
            transition: 'all 0.12s',
          }}
          aria-label={t(locale, 'modules.ariaScan')}
        >
          <RefreshCw size={14} />
          {t(locale, 'modules.scan')}
        </button>
      </div>

      {loading ? (
        <LoadingState message={t(locale, 'common.loading')} />
      ) : (modules ?? []).length === 0 ? (
        <EmptyState title={t(locale, 'modules.emptyState')} />
      ) : (
        <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.xl}px` }}>
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))',
            gap: SPACING.md,
          }}>
            {(modules ?? []).map((mod, i) => (
              <motion.div
                key={mod.id}
                initial={prefersReducedMotion ? undefined : { opacity: 0, y: 12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 70, damping: 13, delay: i * 0.04 }}
                className="doppelrand-outer"
              >
                <div className="doppelrand-inner" style={{ padding: `${SPACING.lg}px` }}>
                  {/* Header: name + status */}
                  <div style={{ display: 'flex', alignItems: 'flex-start', gap: SPACING.sm, marginBottom: SPACING.sm }}>
                    {/* Initial icon */}
                    <div style={{
                      width: 36, height: 36, borderRadius: BORDER_RADIUS.lg,
                      background: 'var(--surface)',
                      border: '1px solid var(--border)',
                      display: 'flex', alignItems: 'center', justifyContent: 'center',
                      fontSize: 15, fontWeight: 700, flexShrink: 0,
                      color: mod.enabled ? 'var(--primary)' : 'var(--text-disabled)',
                    }}>
                      {mod.name.charAt(0).toUpperCase()}
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{
                        fontSize: FONT_SIZE.md, fontWeight: 600, color: 'var(--text)',
                        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                      }}>
                        {mod.name}
                      </div>
                      <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', fontFamily: 'var(--font-mono)', marginTop: 1 }}>
                        {mod.id} v{mod.version}
                      </div>
                    </div>
                    <span style={{
                      fontSize: FONT_SIZE.xs, padding: '2px 8px', borderRadius: BORDER_RADIUS.pill,
                      background: mod.enabled ? 'var(--primary-soft)' : 'var(--surface)',
                      color: mod.enabled ? 'var(--primary)' : 'var(--text-disabled)',
                      fontWeight: 600, flexShrink: 0,
                    }}>
                      {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
                    </span>
                  </div>

                  {/* Description */}
                  {mod.description && (
                    <div style={{
                      fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', lineHeight: 1.45,
                      marginBottom: SPACING.md, display: '-webkit-box',
                      WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden',
                    }}>
                      {mod.description}
                    </div>
                  )}

                  {/* Actions */}
                  <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, marginTop: 'auto' }}>
                    <button
                      onClick={() => handleToggle(mod)}
                      style={{
                        display: 'inline-flex', alignItems: 'center', gap: 4,
                        padding: '5px 10px', borderRadius: BORDER_RADIUS.md,
                        background: 'var(--surface)', border: '1px solid var(--border)',
                        color: mod.enabled ? 'var(--primary)' : 'var(--text-secondary)',
                        cursor: 'pointer', fontSize: FONT_SIZE.xs,
                        transition: 'all 0.12s',
                      }}
                      aria-label={mod.enabled ? t(locale, 'modules.ariaDisable').replace('{name}', mod.name) : t(locale, 'modules.ariaEnable').replace('{name}', mod.name)}
                      title={mod.enabled ? t(locale, 'modules.ariaDisable').replace('{name}', mod.name) : t(locale, 'modules.ariaEnable').replace('{name}', mod.name)}
                    >
                      <Power size={12} />
                      {mod.enabled ? t(locale, 'workshop.disable') || '禁用' : t(locale, 'workshop.enable') || '启用'}
                    </button>
                    <button
                      onClick={() => handleUninstall(mod)}
                      style={{
                        display: 'inline-flex', alignItems: 'center', gap: 4,
                        padding: '5px 10px', borderRadius: BORDER_RADIUS.md,
                        background: 'transparent', border: '1px solid transparent',
                        color: 'var(--text-disabled)', cursor: 'pointer', fontSize: FONT_SIZE.xs,
                        transition: 'all 0.12s',
                      }}
                      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--danger)'; (e.currentTarget as HTMLElement).style.borderColor = 'var(--danger)'; }}
                      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--text-disabled)'; (e.currentTarget as HTMLElement).style.borderColor = 'transparent'; }}
                      aria-label={t(locale, 'modules.ariaUninstall').replace('{name}', mod.name)}
                      title={t(locale, 'modules.ariaUninstall').replace('{name}', mod.name)}
                    >
                      <Trash2 size={12} />
                      {t(locale, 'common.uninstall') || '卸载'}
                    </button>
                  </div>
                </div>
              </motion.div>
            ))}
          </div>
        </div>
      )}

      {/* Uninstall confirmation dialog */}
      {uninstallTarget && (
        <ConfirmDialog
          open={!!uninstallTarget}
          title={t(locale, 'modules.confirmUninstall')}
          message={t(locale, 'modules.confirmUninstallDesc').replace('{name}', uninstallTarget.name)}
          onConfirm={() => { doUninstall(); }}
          onCancel={() => setUninstallTarget(null)}
          confirmLabel={t(locale, 'common.uninstall') || '卸载'}
          cancelLabel={t(locale, 'common.cancel') || '取消'}
          danger
        />
      )}
    </motion.div>
  );
}
