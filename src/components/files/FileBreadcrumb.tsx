'use client';

import { Star } from 'lucide-react';

interface FileBreadcrumbProps {
  segments: string[];
  onNavigate: (path: string) => void;
  isFavorite?: boolean;
  onToggleFavorite?: () => void;
}

export default function FileBreadcrumb({ segments, onNavigate, isFavorite, onToggleFavorite }: FileBreadcrumbProps) {
  if (segments.length === 0) return null;

  return (
    <nav
      aria-label="Breadcrumb"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: '8px 12px',
        fontSize: 13,
        color: 'var(--text, #f2f2ea)',
        overflow: 'hidden',
      }}
    >
      {segments.map((segment, idx) => {
        // Build cumulative path
        const pathSoFar = '/' + segments.slice(0, idx + 1).join('/');
        const isLast = idx === segments.length - 1;

        return (
          <span key={pathSoFar} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            {idx > 0 && (
              <span style={{ color: 'var(--text-faint, #62655a)', fontSize: 12 }}>›</span>
            )}
            {isLast ? (
              <span style={{ color: 'var(--accent, #cdf24b)', fontWeight: 600 }}>{segment}</span>
            ) : (
              <button
                onClick={() => onNavigate(pathSoFar)}
                style={{
                  background: 'none',
                  border: 'none',
                  color: 'var(--text-dim, #9b9d8c)',
                  cursor: 'pointer',
                  padding: '2px 4px',
                  borderRadius: 3,
                  fontSize: 13,
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
                {segment}
              </button>
            )}
          </span>
        );
      })}

      {/* Favorite toggle */}
      {onToggleFavorite && (
        <button
          onClick={onToggleFavorite}
          style={{
            background: 'none', border: 'none', cursor: 'pointer',
            padding: '2px 4px', marginLeft: 'auto', display: 'flex',
            color: isFavorite ? 'var(--accent,#cdf24b)' : 'var(--text-faint,#62655a)',
          }}
          title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
          aria-label={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
        >
          <Star size={14} fill={isFavorite ? 'currentColor' : 'none'} />
        </button>
      )}
    </nav>
  );
}
