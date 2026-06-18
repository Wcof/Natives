'use client';

import { useState, useEffect, useCallback } from 'react';
import { Rocket } from 'lucide-react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { EmptyState } from '@/components/ui/EmptyState';
import { t, type Locale } from '@/i18n';

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
          fontSize: 11,
          color: 'var(--text-faint)',
          padding: '4px 10px',
          background: 'var(--vibe-btn-bg)',
          borderRadius: 6,
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
            borderRadius: 6,
            color: 'var(--text)',
            fontSize: 13,
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
          borderRadius: 10,
          marginBottom: 20,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 16,
        }}>
          <div>
            <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--text)', marginBottom: 4 }}>
              <Rocket size={16} style={{ display: 'inline', verticalAlign: 'middle', marginRight: 4 }} /> {t(locale, 'store.comingSoon')}
            </div>
            <div style={{ fontSize: 12, color: 'var(--text-dim)' }}>
              {t(locale, 'store.comingSoonDesc')}
            </div>
          </div>
          <button
            className="btn btn-primary"
            onClick={handleNavigateToWorkshop}
            style={{ flexShrink: 0, fontSize: 12 }}
          >
            {t(locale, 'store.goToWorkshop')}
          </button>
        </div>

        {/* Installed modules */}
        <div style={{ marginBottom: 16 }}>
          <h2 style={{
            fontSize: 13, fontWeight: 600,
            color: 'var(--vibe-btn-text)',
            textTransform: 'uppercase',
            letterSpacing: 1,
            marginBottom: 12,
          }}>
            {t(locale, 'store.allModules')}
          </h2>
        </div>

        {loading ? (
          <div style={{ padding: '40px 0', textAlign: 'center', color: 'var(--text-faint)', fontSize: 13 }}>
            {t(locale, 'common.loading')}
          </div>
        ) : filteredModules.length === 0 ? (
          <EmptyState title={t(locale, 'store.noModules')} />
        ) : (
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
            gap: 12,
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
        borderRadius: 8,
        padding: '14px 14px 12px',
        transition: 'all 0.12s',
        borderColor: hovered ? 'var(--accent)' : undefined,
        cursor: 'default',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 10, marginBottom: 8 }}>
        {/* Module icon placeholder */}
        <div style={{
          width: 36, height: 36, borderRadius: 8,
          background: 'var(--vibe-btn-bg)',
          border: '1px solid var(--vibe-btn-border)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 16, flexShrink: 0,
        }}>
          {mod.name.charAt(0).toUpperCase()}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{
            fontSize: 13, fontWeight: 600, color: 'var(--text)',
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}>
            {mod.name}
          </div>
          <div style={{ fontSize: 10, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)', marginTop: 1 }}>
            v{mod.version}
          </div>
        </div>
        <span style={{
          fontSize: 9, padding: '1px 5px', borderRadius: 3,
          background: mod.enabled ? 'var(--accent-soft)' : 'var(--vibe-btn-bg)',
          color: mod.enabled ? 'var(--accent)' : 'var(--text-faint)',
          fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.5, flexShrink: 0,
        }}>
          {mod.enabled ? t(locale, 'workshop.enabled') : t(locale, 'workshop.disabled')}
        </span>
      </div>

      {mod.description && (
        <div style={{ fontSize: 11, color: 'var(--text-dim)', lineHeight: 1.4, marginBottom: 8 }}>
          {mod.description}
        </div>
      )}

      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        fontSize: 10, color: 'var(--text-faint)',
      }}>
        <span style={{ fontFamily: 'var(--font-mono)' }}>{mod.id}</span>
        <span style={{
          padding: '2px 6px',
          borderRadius: 3,
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
