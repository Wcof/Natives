'use client';

import { useEffect, useRef, useState } from 'react';
import { Star, Monitor } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { ProjectBadge } from '@/types/file';

const BADGE_LABELS: Record<string, string> = {
  node: 'Node', web: 'Web', python: 'Py', rust: 'Rust', go: 'Go', git: 'Git',
};

const BADGE_COLORS: Record<string, { bg: string; text: string }> = {
  node: { bg: '#33993320', text: '#339933' },
  web: { bg: '#E44D2620', text: '#E44D26' },
  python: { bg: '#3776AB20', text: '#3776AB' },
  rust: { bg: '#DEA58420', text: '#DEA584' },
  go: { bg: '#00ADD820', text: '#00ADD8' },
  git: { bg: '#F0503320', text: '#F05033' },
};

interface FileBreadcrumbProps {
  segments: string[];
  onNavigate: (path: string) => void;
  isFavorite?: boolean;
  onToggleFavorite?: () => void;
  projectBadge?: ProjectBadge | null;
}

export default function FileBreadcrumb({ segments, onNavigate, isFavorite, onToggleFavorite, projectBadge }: FileBreadcrumbProps) {
  const [locale, setLocale] = useState<Locale>('zh');
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // Auto-scroll to end on mount / segment change
  useEffect(() => {
    if (scrollRef.current) {
      requestAnimationFrame(() => {
        scrollRef.current!.scrollLeft = scrollRef.current!.scrollWidth;
      });
    }
  }, [segments]);

  if (segments.length === 0 || (segments.length === 1 && segments[0] === '')) {
    return (
      <nav aria-label={t(locale, 'fileBrowser.breadcrumbLabel')} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 12px', fontSize: 13 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 4, color: 'var(--accent, #cdf24b)', fontWeight: 600 }}>
          <Monitor size={14} /> /
        </span>
        {projectBadge && BADGE_COLORS[projectBadge] && (
          <span style={{ fontSize: 10, padding: '2px 7px', borderRadius: 20, background: BADGE_COLORS[projectBadge].bg, color: BADGE_COLORS[projectBadge].text }}>
            {BADGE_LABELS[projectBadge]}
          </span>
        )}
      </nav>
    );
  }

  return (
    <nav
      aria-label={t(locale, 'fileBrowser.breadcrumbLabel')}
      ref={scrollRef}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: '8px 12px',
        fontSize: 13,
        color: 'var(--text, #f2f2ea)',
        overflowX: 'auto',
        scrollbarWidth: 'none',
        whiteSpace: 'nowrap',
      }}
    >
      {/* Root segment with monitor icon */}
      <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        {segments.length > 0 ? (
          <button
            onClick={() => onNavigate('/')}
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              color: 'var(--text-dim, #9b9d8c)', padding: '2px 4px', borderRadius: 3,
              fontSize: 13, display: 'flex', alignItems: 'center', gap: 3,
              transition: 'color 0.1s, background 0.1s',
            }}
            onMouseEnter={(e) => {
              (e.currentTarget as HTMLElement).style.color = 'var(--text, #f2f2ea)';
              (e.currentTarget as HTMLElement).style.background = 'var(--bg-2, #131410)';
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLElement).style.color = 'var(--text-dim, #9b9d8c)';
              (e.currentTarget as HTMLElement).style.background = 'none';
            }}
          >
            <Monitor size={13} style={{ color: 'var(--accent, #cdf24b)' }} />
          </button>
        ) : (
          <Monitor size={13} style={{ color: 'var(--accent, #cdf24b)' }} />
        )}
      </span>

      {segments.map((segment, idx) => {
        const pathSoFar = '/' + segments.slice(0, idx + 1).join('/');
        const isLast = idx === segments.length - 1;

        return (
          <span key={pathSoFar} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            <span style={{ color: 'var(--text-faint, #62655a)', fontSize: 12 }}>›</span>
            {isLast ? (
              <span style={{
                color: 'var(--accent, #cdf24b)', fontWeight: 600,
                fontFamily: 'var(--font-display)',
                background: 'var(--accent-soft, #cdf24b1f)',
                padding: '2px 7px', borderRadius: 6,
              }}>
                {segment}
              </span>
            ) : (
              <button
                onClick={() => onNavigate(pathSoFar)}
                style={{
                  background: 'none', border: 'none',
                  color: 'var(--text-dim, #9b9d8c)',
                  cursor: 'pointer', padding: '2px 4px', borderRadius: 3,
                  fontSize: 13, transition: 'color 0.1s, background 0.1s',
                }}
                onMouseEnter={(e) => {
                  (e.currentTarget as HTMLElement).style.color = 'var(--text, #f2f2ea)';
                  (e.currentTarget as HTMLElement).style.background = 'var(--bg-2, #131410)';
                }}
                onMouseLeave={(e) => {
                  (e.currentTarget as HTMLElement).style.color = 'var(--text-dim, #9b9d8c)';
                  (e.currentTarget as HTMLElement).style.background = 'none';
                }}
              >
                {segment}
              </button>
            )}
          </span>
        );
      })}

      {/* Project badge */}
      {projectBadge && BADGE_COLORS[projectBadge] && (
        <span style={{
          fontSize: 9, fontWeight: 600, padding: '1px 5px', borderRadius: 3,
          background: BADGE_COLORS[projectBadge].bg, color: BADGE_COLORS[projectBadge].text,
          marginLeft: 4, lineHeight: '16px', flexShrink: 0,
        }}>
          {BADGE_LABELS[projectBadge]}
        </span>
      )}

      {/* Favorite toggle */}
      {onToggleFavorite && (
        <button
          onClick={onToggleFavorite}
          style={{
            background: 'none', border: 'none', cursor: 'pointer',
            padding: '2px 4px', marginLeft: 'auto', display: 'flex', flexShrink: 0,
            color: isFavorite ? 'var(--accent,#cdf24b)' : 'var(--text-faint,#62655a)',
          }}
          title={t(locale, isFavorite ? 'fileBrowser.removeFromFavorites' : 'fileBrowser.addToFavorites')}
          aria-label={t(locale, isFavorite ? 'fileBrowser.removeFromFavorites' : 'fileBrowser.addToFavorites')}
        >
          <Star size={14} fill={isFavorite ? 'currentColor' : 'none'} />
        </button>
      )}
    </nav>
  );
}
