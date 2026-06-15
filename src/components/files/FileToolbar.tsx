'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';

interface FileToolbarProps {
  viewMode: 'grid' | 'list';
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  showHidden: boolean;
  searchQuery: string;
  onViewModeChange: (mode: 'grid' | 'list') => void;
  onSortChange: (sortBy: 'name' | 'mtime' | 'size') => void;
  onSortDirChange: (dir: 'asc' | 'desc') => void;
  onShowHiddenChange: (show: boolean) => void;
  onSearchChange: (query: string) => void;
  onRefresh?: () => void;
  onNewFile?: () => void;
  onNewFolder?: () => void;
}

export default function FileToolbar({
  viewMode, sortBy, sortDir, showHidden, searchQuery,
  onViewModeChange, onSortChange, onSortDirChange,
  onShowHiddenChange, onSearchChange,
  onRefresh, onNewFile, onNewFolder,
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
      gap: 8,
      padding: '6px 12px',
      borderBottom: '1px solid var(--border, #262920)',
      flexWrap: 'wrap',
    }}>
      {/* View mode toggle */}
      <div className="segmented-control" style={{ display: 'flex' }}>
        <button
          className={`seg-item ${viewMode === 'grid' ? 'active' : ''}`}
          onClick={() => onViewModeChange('grid')}
          title={t(locale, 'fileBrowser.gridView')}
        >
          ▦
        </button>
        <button
          className={`seg-item ${viewMode === 'list' ? 'active' : ''}`}
          onClick={() => onViewModeChange('list')}
          title={t(locale, 'fileBrowser.listView')}
        >
          ☰
        </button>
      </div>

      {/* Sort */}
      <select
        className="input"
        value={sortBy}
        onChange={(e) => onSortChange(e.target.value as 'name' | 'mtime' | 'size')}
        style={{ width: 100, fontSize: 12, padding: '4px 6px' }}
      >
        <option value="name">{t(locale, 'fileBrowser.name')}</option>
        <option value="mtime">{t(locale, 'fileBrowser.modified')}</option>
        <option value="size">{t(locale, 'fileBrowser.size')}</option>
      </select>

      {/* Sort direction */}
      <button
        className="btn btn-ghost"
        onClick={() => onSortDirChange(sortDir === 'asc' ? 'desc' : 'asc')}
        style={{ fontSize: 14, padding: '2px 6px' }}
        title={sortDir === 'asc' ? 'Ascending' : 'Descending'}
      >
        {sortDir === 'asc' ? '↑' : '↓'}
      </button>

      {/* Show hidden toggle */}
      <label className="switch" style={{ marginLeft: 4 }}>
        <input
          type="checkbox"
          checked={showHidden}
          onChange={(e) => onShowHiddenChange(e.target.checked)}
          style={{ display: 'none' }}
        />
        <span className={`switch-track ${showHidden ? 'active' : ''}`}>
          <span className="switch-knob" />
        </span>
        <span style={{ marginLeft: 6, fontSize: 11, color: 'var(--text-dim, #9b9d8c)' }}>
          Hidden
        </span>
      </label>

      {/* Spacer */}
      <div style={{ flex: 1 }} />

      {/* Actions */}
      {onNewFile && (
        <button className="btn btn-ghost" onClick={onNewFile} style={{ fontSize: 11, padding: '3px 8px' }} title="New File">
          + File
        </button>
      )}
      {onNewFolder && (
        <button className="btn btn-ghost" onClick={onNewFolder} style={{ fontSize: 11, padding: '3px 8px' }} title="New Folder">
          + Folder
        </button>
      )}
      {onRefresh && (
        <button className="btn btn-ghost" onClick={onRefresh} style={{ fontSize: 11, padding: '3px 8px' }} title="Refresh">
          ↻
        </button>
      )}

      {/* Search */}
      <input
        className="input"
        type="text"
        placeholder="Search files..."
        value={searchQuery}
        onChange={(e) => onSearchChange(e.target.value)}
        style={{ width: 180, fontSize: 12, padding: '4px 8px' }}
      />
    </div>
  );
}
