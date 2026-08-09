'use client';

/**
 * FileArea — 文件浏览器文件区（F3-04 布局子组件，ARCH-002）。
 *
 * 职责：拖拽覆盖层 + 加载骨架 + grid/list 切换渲染 + 空白区点击/右键路由。
 * 持有滚动容器 / grid 容器 ref 的装配（ref 由父层 FileBrowser 持有并传入），
 * 业务状态与动作全部经 props 注入，本组件不产生任何文件逻辑。
 */

import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import type { VirtualFileViewHandle } from '@/lib/preview/contracts';
import FileGrid from '../FileGrid';
import FileList from '../FileList';
import Skeleton from '@/components/ui/Skeleton';

export interface FileDragHandlers {
  onDragEnter: (e: React.DragEvent) => void;
  onDragLeave: (e: React.DragEvent) => void;
  onDragOver: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent) => void;
}

export interface FileAreaProps {
  locale: Locale;
  viewMode: 'grid' | 'list';
  loading: boolean;
  /** 过滤后的渲染列表 */
  entries: FileEntry[];
  isDragging: boolean;
  dragHandlers: FileDragHandlers;
  // Selection (item-level)
  selectedIndex: number;
  selectedPaths: Set<string>;
  onSelect: (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => void;
  onItemContextMenu: (e: React.MouseEvent, entry: FileEntry) => void;
  // Blank area routing
  onBlankClick: () => void;
  onBlankContextMenu: (x: number, y: number) => void;
  // Grid / list props
  gridSize: 'sm' | 'md' | 'lg';
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  showDir: boolean;
  onSort: (by: 'name' | 'mtime' | 'size') => void;
  onEditRequest: (entry: FileEntry) => void;
  favorites: string[];
  onFavoriteToggle: (entry: FileEntry) => void;
  cutPaths?: Set<string>;
  onMoveDrop: (sourcePaths: string[], destDir: string) => void;
  dragPaths?: string[];
  flashPaths?: Set<string>;
  // Refs & virtual-view seam
  areaRef: React.RefObject<HTMLDivElement | null>;
  gridContainerRef: React.RefObject<HTMLDivElement | null>;
  scrollContainerRef: React.RefObject<HTMLElement | null>;
  onViewHandleReady: (handle: VirtualFileViewHandle) => void;
}

export default function FileArea({
  locale,
  viewMode,
  loading,
  entries,
  isDragging,
  dragHandlers,
  selectedIndex,
  selectedPaths,
  onSelect,
  onItemContextMenu,
  onBlankClick,
  onBlankContextMenu,
  gridSize,
  sortBy,
  sortDir,
  showDir,
  onSort,
  onEditRequest,
  favorites,
  onFavoriteToggle,
  cutPaths,
  onMoveDrop,
  dragPaths,
  flashPaths,
  areaRef,
  gridContainerRef,
  scrollContainerRef,
  onViewHandleReady,
}: FileAreaProps) {
  return (
    // File area — drop zone covers entire height including empty space
    <div
      ref={areaRef}
      {...dragHandlers}
      style={{ flex: 1, overflow: 'auto', position: 'relative' }}
      role="listbox"
      aria-label={t(locale, 'fileBrowser.ariaLabelFiles')}
      tabIndex={0}
      onClick={(e) => {
        const target = e.target as HTMLElement;
        if (!target.closest('[data-file-entry]')) {
          onBlankClick();
        }
      }}
      onContextMenu={(e) => {
        // Blank area right-click — only if not on a file/dir element
        const target = e.target as HTMLElement;
        if (!target.closest('[data-file-entry]')) {
          e.preventDefault();
          onBlankContextMenu(e.clientX, e.clientY);
        }
      }}
    >
      {/* Drop overlay — fills entire area including empty space below files */}
      {isDragging && (
        <div
          style={{
            position: 'absolute',
            inset: 4,
            zIndex: 20,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            pointerEvents: 'none',
            border: '2px dashed var(--primary)',
            borderRadius: 'var(--radius, 4px)',
            background: 'var(--accent-soft, rgba(205,242,75,0.08))',
            color: 'var(--primary)',
            fontSize: FONT_SIZE.lg,
            fontWeight: 600,
          }}
        >
          {t(locale, 'fileBrowser.dropHere')}
        </div>
      )}
      {loading ? (
        <div style={{ padding: viewMode === 'grid' ? 12 : 0 }}>
          {viewMode === 'grid' ? (
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))',
                gap: SPACING.sm,
              }}
            >
              {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} variant="card" />)}
            </div>
          ) : (
            <Skeleton variant="table" lines={10} />
          )}
        </div>
      ) : viewMode === 'grid' ? (
        <FileGrid
          ref={gridContainerRef}
          entries={entries}
          onSelect={onSelect}
          onContextMenu={onItemContextMenu}
          selectedIndex={selectedIndex}
          selectedPaths={selectedPaths}
          gridSize={gridSize}
          onEditRequest={onEditRequest}
          favorites={favorites}
          onFavoriteToggle={onFavoriteToggle}
          cutPaths={cutPaths}
          onMoveDrop={onMoveDrop}
          dragPaths={dragPaths}
          flashPaths={flashPaths}
          scrollContainerRef={scrollContainerRef}
          onViewHandleReady={onViewHandleReady}
        />
      ) : (
        <FileList
          entries={entries}
          sortBy={sortBy}
          sortDir={sortDir}
          onSort={onSort}
          onSelect={onSelect}
          onContextMenu={onItemContextMenu}
          showDir={showDir}
          selectedIndex={selectedIndex}
          selectedPaths={selectedPaths}
          onEditRequest={onEditRequest}
          favorites={favorites}
          onFavoriteToggle={onFavoriteToggle}
          cutPaths={cutPaths}
          onMoveDrop={onMoveDrop}
          dragPaths={dragPaths}
          flashPaths={flashPaths}
          scrollContainerRef={scrollContainerRef}
          onViewHandleReady={onViewHandleReady}
        />
      )}
    </div>
  );
}
