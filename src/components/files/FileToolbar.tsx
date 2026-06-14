'use client';

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
}

export default function FileToolbar({
  viewMode, sortBy, sortDir, showHidden, searchQuery,
  onViewModeChange, onSortChange, onSortDirChange,
  onShowHiddenChange, onSearchChange,
}: FileToolbarProps) {
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
          title="Grid view"
        >
          ▦
        </button>
        <button
          className={`seg-item ${viewMode === 'list' ? 'active' : ''}`}
          onClick={() => onViewModeChange('list')}
          title="List view"
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
        <option value="name">Name</option>
        <option value="mtime">Modified</option>
        <option value="size">Size</option>
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
