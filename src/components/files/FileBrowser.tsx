'use client';

import { useState, useEffect, useCallback } from 'react';
import { type FileEntry } from '@/types/file';
import FileGrid from './FileGrid';
import FileList from './FileList';
import FileBreadcrumb from './FileBreadcrumb';
import FileToolbar from './FileToolbar';
import FileContextMenu from './FileContextMenu';

interface FileBrowserProps {
  onFileSelect?: (entry: FileEntry) => void;
}

export default function FileBrowser({ onFileSelect }: FileBrowserProps) {
  const [currentPath, setCurrentPath] = useState('/');
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [viewMode, setViewMode] = useState<'grid' | 'list'>('grid');
  const [sortBy, setSortBy] = useState<'name' | 'mtime' | 'size'>('name');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');
  const [showHidden, setShowHidden] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entry: FileEntry } | null>(null);

  const loadEntries = useCallback(async () => {
    setLoading(true);
    try {
      const api = window.nativesAPI;

      if (api?.fs?.listDir) {
        // Use IPC (Electron)
        const options = { sortBy, sortDir, showHidden };
        const data = await api.fs.listDir(currentPath, options);
        setEntries(data || []);
      } else {
        // Browser dev mode: empty state
        setEntries([]);
      }
    } catch {
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [currentPath, sortBy, sortDir, showHidden]);

  useEffect(() => {
    loadEntries();
  }, [loadEntries]);

  const handleSelect = (entry: FileEntry) => {
    if (entry.isDir) {
      setCurrentPath(entry.path);
    } else {
      onFileSelect?.(entry);
    }
  };

  const handleNavigate = (path: string) => {
    setCurrentPath(path);
  };

  const handleSort = (newSortBy: 'name' | 'mtime' | 'size') => {
    if (newSortBy === sortBy) {
      setSortDir(sortDir === 'asc' ? 'desc' : 'asc');
    } else {
      setSortBy(newSortBy);
      setSortDir('asc');
    }
  };

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const segments = currentPath.split('/').filter(Boolean);

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      background: 'var(--bg, #0b0c0a)',
    }}>
      {/* Navigation */}
      <FileBreadcrumb segments={segments.length > 0 ? segments : ['/']} onNavigate={handleNavigate} />
      <FileToolbar
        viewMode={viewMode}
        sortBy={sortBy}
        sortDir={sortDir}
        showHidden={showHidden}
        searchQuery={searchQuery}
        onViewModeChange={setViewMode}
        onSortChange={handleSort}
        onSortDirChange={setSortDir}
        onShowHiddenChange={setShowHidden}
        onSearchChange={setSearchQuery}
      />

      {/* File area */}
      <div style={{ flex: 1, overflow: 'auto' }}>
        {loading ? (
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%', color: 'var(--text-faint, #62655a)' }}>
            Loading...
          </div>
        ) : viewMode === 'grid' ? (
          <FileGrid entries={entries} onSelect={handleSelect} onContextMenu={handleContextMenu} />
        ) : (
          <FileList
            entries={entries}
            sortBy={sortBy}
            sortDir={sortDir}
            onSort={handleSort}
            onSelect={handleSelect}
            onContextMenu={handleContextMenu}
          />
        )}
      </div>

      {/* Context menu */}
      {contextMenu && (
        <FileContextMenu
          entry={contextMenu.entry}
          x={contextMenu.x}
          y={contextMenu.y}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
