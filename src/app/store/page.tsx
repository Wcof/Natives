'use client';

import { useState, useEffect, useCallback } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { Rocket, Search, Package } from 'lucide-react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface ModuleInfo {
  id: string;
  name: string;
  version: string;
  enabled: number;
  state: string;
  description?: string;
  author?: string;
}

export default function StorePage() {
  const prefersReducedMotion = useReducedMotion();
  const [searchQuery, setSearchQuery] = useState('');
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function init() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* browser dev mode */ }
    }
    init();
  }, []);

  const { data: modules, loading, error, reload: loadModules } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (api?.module?.list) {
      const result = await api.module.list();
      if (Array.isArray(result)) return result as ModuleInfo[];
    }
    return [];
  }, []);

  const filteredModules = (modules ?? []).filter((m) => {
    if (!searchQuery) return true;
    const q = searchQuery.toLowerCase();
    return (
      m.name.toLowerCase().includes(q) ||
      m.id.toLowerCase().includes(q) ||
      (m.description && m.description.toLowerCase().includes(q))
    );
  });

  const handleNavigateToWorkshop = () => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: '__workshop__' }));
  };

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 60, damping: 16, mass: 1 }}
      style={{ height: '100%', overflow: 'auto' }}
    >
      {/* Header — title + count */}
      <div style={{
        padding: `${SPACING.md}px ${SPACING.xl}px`,
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        borderBottom: '1px solid var(--border)',
      }}>
        <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)' }}>
          {t(locale, 'store.title') || '模块商店'}
        </div>
        <div style={{
          fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)',
          padding: '2px 10px', background: 'var(--surface)',
          borderRadius: BORDER_RADIUS.pill,
          border: '1px solid var(--border)',
        }}>
          {t(locale, 'store.moduleCount').replace('{count}', String(modules?.length ?? 0))}
        </div>
      </div>

      {/* Search bar */}
      <div style={{ padding: `${SPACING.md}px ${SPACING.xl}px` }}>
        <div style={{
          position: 'relative',
          display: 'flex', alignItems: 'center',
        }}>
          <Search size={14} style={{ position: 'absolute', left: 10, color: 'var(--text-disabled)', pointerEvents: 'none' }} />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder={t(locale, 'store.searchPlaceholder')}
            style={{
              width: '100%',
              padding: '8px 10px 8px 32px',
              background: 'var(--surface)',
              border: '1px solid var(--border)',
              borderRadius: BORDER_RADIUS.lg,
              color: 'var(--text)',
              fontSize: FONT_SIZE.md,
              outline: 'none',
              transition: 'border-color 0.12s',
            }}
            onFocus={(e) => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--primary)'; }}
            onBlur={(e) => { (e.currentTarget as HTMLElement).style.borderColor = 'var(--border)'; }}
          />
        </div>
      </div>

      {/* Coming soon banner */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.md}px` }}>
        <motion.div
          initial={prefersReducedMotion ? undefined : { opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 70, damping: 13, delay: 0.1 }}
          style={{
            padding: `${SPACING.lg}px ${SPACING.xl}px`,
            borderRadius: BORDER_RADIUS.xl,
            border: '1px solid var(--primary-soft)',
            background: 'linear-gradient(135deg, var(--primary-soft) 0%, transparent 60%)',
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
            gap: SPACING.lg,
          }}
        >
          <div>
            <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              <Rocket size={14} style={{ display: 'inline', verticalAlign: 'middle', marginRight: 6, color: 'var(--primary)' }} />
              {t(locale, 'store.comingSoon')}
            </div>
            <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', lineHeight: 1.4 }}>
              {t(locale, 'store.comingSoonDesc')}
            </div>
          </div>
          <button
            onClick={handleNavigateToWorkshop}
            style={{
              flexShrink: 0,
              display: 'inline-flex', alignItems: 'center', gap: 6,
              padding: '7px 14px',
              borderRadius: BORDER_RADIUS.md,
              background: 'var(--primary)', border: 'none',
              color: '#FFFFFF', fontWeight: 600,
              fontSize: FONT_SIZE.sm, cursor: 'pointer',
              transition: 'filter 0.12s',
            }}
          >
            <Package size={14} />
            {t(locale, 'store.goToWorkshop')}
          </button>
        </motion.div>
      </div>

      {/* Module grid */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.xl}px` }}>
        {loading ? (
          <LoadingState message={t(locale, 'common.loading')} />
        ) : filteredModules.length === 0 ? (
          <EmptyState
            title={searchQuery ? (t(locale, 'store.noSearchResults') || '无搜索结果') : (t(locale, 'store.noModules') || '暂无模块')}
            description={searchQuery ? (t(locale, 'store.tryDifferentSearch') || '尝试其他关键词') : undefined}
          />
        ) : (
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
            gap: SPACING.md,
          }}>
            {filteredModules.map((mod, i) => (
              <StoreModuleCard key={mod.id} module={mod} locale={locale} index={i} />
            ))}
          </div>
        )}
      </div>
    </motion.div>
  );
}

// ── Store Module Card ──

function StoreModuleCard({ module: mod, locale, index }: { module: ModuleInfo; locale: Locale; index: number }) {
  const prefersReducedMotion = useReducedMotion();
  const [hovered, setHovered] = useState(false);

  const accentVar = mod.enabled ? 'var(--primary)' : 'var(--text-disabled)';
  const accentSoftVar = mod.enabled ? 'var(--primary-soft)' : 'var(--surface)';

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 70, damping: 13, delay: index * 0.03 }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      className="doppelrand-outer"
      style={{
        transition: 'border-color 0.15s, box-shadow 0.15s',
        borderColor: hovered ? accentVar : undefined,
      }}
    >
      <div className="doppelrand-inner" style={{
        padding: '16px 16px 14px',
        cursor: 'default',
      }}>
        {/* Icon + name row */}
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: 10, marginBottom: SPACING.sm }}>
          <div style={{
            width: 38, height: 38, borderRadius: BORDER_RADIUS.lg,
            background: accentSoftVar,
            border: '1px solid var(--border)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            fontSize: 16, fontWeight: 700, flexShrink: 0,
            color: accentVar,
            transition: 'all 0.15s',
            transform: hovered ? 'scale(1.05) rotate(3deg)' : 'none',
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
              v{mod.version}
            </div>
          </div>
          <span style={{
            fontSize: FONT_SIZE.xs, padding: '1px 6px', borderRadius: BORDER_RADIUS.pill,
            background: mod.enabled ? 'var(--primary-soft)' : 'var(--surface)',
            color: mod.enabled ? 'var(--primary)' : 'var(--text-disabled)',
            fontWeight: 600, letterSpacing: '0.02em', flexShrink: 0,
          }}>
            {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
          </span>
        </div>

        {/* Description */}
        {mod.description && (
          <div style={{
            fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', lineHeight: 1.45,
            marginBottom: SPACING.sm, display: '-webkit-box',
            WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden',
          }}>
            {mod.description}
          </div>
        )}

        {/* Footer */}
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)',
          paddingTop: SPACING.xs,
          borderTop: '1px solid var(--border)',
        }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: '0.625rem' }}>{mod.id}</span>
          <span style={{
            padding: '1px 6px', borderRadius: BORDER_RADIUS.sm,
            background: 'var(--surface)',
            border: '1px solid var(--border)',
            fontSize: '0.625rem',
          }}>
            {t(locale, 'store.installed')}
          </span>
        </div>
      </div>
    </motion.div>
  );
}
