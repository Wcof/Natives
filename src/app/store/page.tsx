'use client';

import { useState, useEffect, useCallback } from 'react';
import { Rocket } from 'lucide-react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';
import { t, type Locale } from '@/i18n';
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

export default function StorePage() {
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
    <div style={{ height: '100%', overflow: 'auto' }}>
      {/* Count badge — minimal, no title (shown in top Header) */}
      <div style={{ padding: '12px 20px', display: 'flex', alignItems: 'center', justifyContent: 'flex-end' }}>
        <div style={{
          fontSize: FONT_SIZE.sm,
          color: 'var(--text-faint)',
          padding: '4px 10px',
          background: 'var(--vibe-btn-bg)',
          borderRadius: BORDER_RADIUS.md,
          border: '1px solid var(--vibe-btn-border)',
        }}>
          {t(locale, 'store.moduleCount').replace('{count}', String(modules?.length ?? 0))}
        </div>
      </div>

      {/* Search bar */}
      <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--vibe-btn-border)' }}>
        <input
          type="text"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder={t(locale, 'store.searchPlaceholder')}
          style={{
            width: '100%',
            padding: '8px 12px',
            background: 'var(--vibe-content-bg)',
            border: '1px solid var(--vibe-btn-border)',
            borderRadius: BORDER_RADIUS.md,
            color: 'var(--text)',
            fontSize: FONT_SIZE.lg,
            outline: 'none',
          }}
        />
      </div>

      <div style={{ padding: '16px 20px' }}>
        {/* Coming soon banner */}
        <div style={{
          padding: '20px 24px',
          background: 'linear-gradient(135deg, var(--accent-soft), transparent)',
          border: '1px solid var(--vibe-btn-border)',
          borderRadius: BORDER_RADIUS.xl,
          marginBottom: 20,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: SPACING.lg,
        }}>
          <div>
            <div style={{ fontSize: FONT_SIZE.xl, fontWeight: 600, color: 'var(--text)', marginBottom: SPACING.xs }}>
              <Rocket size={16} style={{ display: 'inline', verticalAlign: 'middle', marginRight: 4 }} /> {t(locale, 'store.comingSoon')}
            </div>
            <div style={{ fontSize: FONT_SIZE.md, color: 'var(--text-dim)' }}>
              {t(locale, 'store.comingSoonDesc')}
            </div>
          </div>
          <button
            className="btn btn-primary"
            onClick={handleNavigateToWorkshop}
            style={{ flexShrink: 0, fontSize: FONT_SIZE.md }}
          >
            {t(locale, 'store.goToWorkshop')}
          </button>
        </div>

        {/* Installed modules */}
        <div style={{ marginBottom: SPACING.lg }}>
          <h2 style={{
            fontSize: FONT_SIZE.lg, fontWeight: 600,
            color: 'var(--vibe-btn-text)',
            textTransform: 'uppercase',
            letterSpacing: 1,
            marginBottom: SPACING.md,
          }}>
            {t(locale, 'store.allModules')}
          </h2>
        </div>

        {loading ? (
          <LoadingState message={t(locale, 'common.loading')} />
        ) : filteredModules.length === 0 ? (
          <EmptyState title={t(locale, 'store.noModules')} />
        ) : (
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
            gap: SPACING.md,
          }}>
            {filteredModules.map((mod) => (
              <StoreModuleCard key={mod.id} module={mod} locale={locale} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Store Module Card ──

function StoreModuleCard({ module: mod, locale }: { module: ModuleInfo; locale: Locale }) {
  const [hovered, setHovered] = useState(false);

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        background: 'var(--vibe-toolbar-bg)',
        border: '1px solid var(--vibe-btn-border)',
        borderRadius: BORDER_RADIUS.lg,
        padding: '14px 14px 12px',
        transition: 'all 0.12s',
        borderColor: hovered ? 'var(--accent)' : undefined,
        cursor: 'default',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 10, marginBottom: SPACING.sm }}>
        {/* Module icon placeholder */}
        <div style={{
          width: 36, height: 36, borderRadius: BORDER_RADIUS.lg,
          background: 'var(--vibe-btn-bg)',
          border: '1px solid var(--vibe-btn-border)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 16, flexShrink: 0,
        }}>
          {mod.name.charAt(0).toUpperCase()}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{
            fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)',
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}>
            {mod.name}
          </div>
          <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', marginTop: 1 }}>
            v{mod.version}
          </div>
        </div>
        <span style={{
          fontSize: 9, padding: '1px 5px', borderRadius: BORDER_RADIUS.sm,
          background: mod.enabled ? 'var(--accent-soft)' : 'var(--vibe-btn-bg)',
          color: mod.enabled ? 'var(--accent)' : 'var(--text-faint)',
          fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5, flexShrink: 0,
        }}>
          {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
        </span>
      </div>

      {mod.description && (
        <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', lineHeight: 1.4, marginBottom: SPACING.sm }}>
          {mod.description}
        </div>
      )}

      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        fontSize: FONT_SIZE.xs, color: 'var(--text-faint)',
      }}>
        <span style={{ fontFamily: 'var(--font-mono)' }}>{mod.id}</span>
        <span style={{
          padding: '2px 6px',
          borderRadius: BORDER_RADIUS.sm,
          background: 'var(--vibe-btn-bg)',
          border: '1px solid var(--vibe-btn-border)',
          fontSize: 9,
          textTransform: 'uppercase',
        }}>
          {t(locale, 'store.installed')}
        </span>
      </div>
    </div>
  );
}
