'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { ArrowLeft, ArrowRight, RefreshCw, Clock, FilePlus, FolderPlus } from 'lucide-react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';

interface FileToolbarProps {
  searchQuery: string;
  recentMode?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onSearchChange: (query: string) => void;
  onRecentModeToggle?: () => void;
  onRefresh?: () => void;
  onNewFile?: () => void;
  onNewFolder?: () => void;
  onBack?: () => void;
  onForward?: () => void;
}

export default function FileToolbar({
  searchQuery, recentMode, canGoBack, canGoForward,
  onSearchChange, onRecentModeToggle, onRefresh,
  onNewFile, onNewFolder, onBack, onForward,
}: FileToolbarProps) {
  const [locale, setLocale] = useState<Locale>('en');
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  return (
    <div style={{
      display: 'flex',
      alignItems: 'center',
      gap: SPACING.sm,
      padding: '6px 12px',
      borderBottom: '1px solid var(--vibe-btn-border)',
      flexWrap: 'wrap',
    }}>
      {/* Navigation: back / forward */}
      {onBack && onForward && (
        <div style={{ display: 'flex', gap: 2 }}>
          <button
            className="btn btn-ghost"
            onClick={onBack}
            disabled={!canGoBack}
            title={t(locale,'fileBrowser.back')}
            style={{ padding: '2px 6px', fontSize: FONT_SIZE.lg, opacity: canGoBack ? 1 : 0.3 }}
          >
            <ArrowLeft size={14} />
          </button>
          <button
            className="btn btn-ghost"
            onClick={onForward}
            disabled={!canGoForward}
            title={t(locale,'fileBrowser.forward')}
            style={{ padding: '2px 6px', fontSize: FONT_SIZE.xl, opacity: canGoForward ? 1 : 0.3 }}
          >
            <ArrowRight size={14} />
          </button>
        </div>
      )}

      {/* Recent mode toggle */}
      {onRecentModeToggle && (
        <button
          className="btn btn-ghost"
          onClick={onRecentModeToggle}
          style={{
            fontSize: FONT_SIZE.lg, padding: '2px 6px',
            color: recentMode ? 'var(--accent)' : undefined,
          }}
          title={t(locale, recentMode ? 'fileBrowser.recentModeTitle' : 'fileBrowser.recentMode')}
        >
          <Clock size={14} />
        </button>
      )}

      {/* Spacer */}
      <div style={{ flex: 1 }} />

      {/* Actions */}
      {onNewFile && (
        <button className="btn btn-ghost" onClick={onNewFile} style={{ fontSize: FONT_SIZE.sm, padding: '3px 8px' }} title={t(locale,'fileBrowser.newFile')}>
          <FilePlus size={12} style={{ marginRight: 4 }} /> {t(locale,'fileBrowser.newFile')}
        </button>
      )}
      {onNewFolder && (
        <button className="btn btn-ghost" onClick={onNewFolder} style={{ fontSize: FONT_SIZE.sm, padding: '3px 8px' }} title={t(locale,'fileBrowser.newFolder')}>
          <FolderPlus size={12} style={{ marginRight: 4 }} /> {t(locale,'fileBrowser.newFolder')}
        </button>
      )}
      {onRefresh && (
        <button className="btn btn-ghost" onClick={onRefresh} style={{ fontSize: FONT_SIZE.sm, padding: '3px 8px' }} title={t(locale,'fileBrowser.refresh')}>
          <RefreshCw size={12} />
        </button>
      )}

      {/* Search */}
      <input
        className="input"
        type="text"
        placeholder={t(locale, 'fileBrowser.searchPlaceholder')}
        value={searchQuery}
        onChange={(e) => onSearchChange(e.target.value)}
        style={{ width: 180, fontSize: FONT_SIZE.md, padding: '4px 8px' }}
      />
    </div>
  );
}
