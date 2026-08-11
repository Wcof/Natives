'use client';

import { ReactNode, useCallback, useEffect, useRef, useState } from 'react';
import { X } from 'lucide-react';

/** Default open width — also used as double-click reset target. */
export const RESIZABLE_RIGHT_PANEL_DEFAULT_WIDTH = 320;
/** Narrowest usable width. */
export const RESIZABLE_RIGHT_PANEL_MIN_WIDTH = 260;
/** Keep at least this much horizontal room for the main workspace. */
const MAIN_CONTENT_FLOOR = 420;

/**
 * 问题12：右栏无任意产品上限（不再 clamp 到 640）。只受「当前视口可用宽」
 * 物理边界约束，防止拖出屏幕；窗口缩小后仅在超出物理边界时收敛。
 */
export function clampResizableRightPanelWidth(
  width: number,
  viewportWidth = typeof window !== 'undefined' ? window.innerWidth : 1280,
): number {
  const maxByViewport = Math.max(
    RESIZABLE_RIGHT_PANEL_MIN_WIDTH,
    viewportWidth - MAIN_CONTENT_FLOOR,
  );
  return Math.max(RESIZABLE_RIGHT_PANEL_MIN_WIDTH, Math.min(maxByViewport, Math.round(width)));
}

/** 当前视口允许的最大右栏宽度（供 aria-valuemax 动态使用）。 */
export function rightPanelViewportMax(
  viewportWidth = typeof window !== 'undefined' ? window.innerWidth : 1280,
): number {
  return Math.max(RESIZABLE_RIGHT_PANEL_MIN_WIDTH, viewportWidth - MAIN_CONTENT_FLOOR);
}

export interface ResizableRightPanelTab {
  id: string;
  label: string;
  icon?: ReactNode;
  title?: string;
}

export interface ResizableRightPanelProps {
  open: boolean;
  width: number;
  onResize: (width: number) => void;
  onClose?: () => void;
  /** Optional plain title when tabs are not used. */
  title?: string;
  /** Optional tab strip; business modules supply their own tab ids. */
  tabs?: ResizableRightPanelTab[];
  activeTabId?: string;
  onTabChange?: (tabId: string) => void;
  /** Extra header slot (e.g. actions) after tabs / title. */
  headerExtra?: ReactNode;
  children?: ReactNode;
  /** Accessible name for the region. */
  ariaLabel?: string;
  className?: string;
  /** When true, body scrolls; default true. */
  scrollBody?: boolean;
  /** Locale-aware resize handle labels. */
  resizeLabel?: string;
  resizeHint?: string;
  closeLabel?: string;
}

/**
 * Minimal shared right rail: size, drag, title/tabs, close, scroll, responsive clamp.
 * Business content (file preview / assistant inspector) stays in adapters.
 */
export default function ResizableRightPanel({
  open,
  width,
  onResize,
  onClose,
  title,
  tabs,
  activeTabId,
  onTabChange,
  headerExtra,
  children,
  ariaLabel,
  className = '',
  scrollBody = true,
  resizeLabel = 'Resize panel',
  resizeHint = 'Drag to resize · double-click to reset',
  closeLabel = 'Close panel',
}: ResizableRightPanelProps) {
  const [isDragging, setIsDragging] = useState(false);
  const widthRef = useRef(width);

  useEffect(() => {
    widthRef.current = width;
  }, [width]);

  useEffect(() => {
    if (!open) return;
    const onWinResize = () => {
      const next = clampResizableRightPanelWidth(widthRef.current);
      if (next !== widthRef.current) onResize(next);
    };
    window.addEventListener('resize', onWinResize);
    return () => window.removeEventListener('resize', onWinResize);
  }, [open, onResize]);

  const handleDragStart = useCallback(
    (e: React.MouseEvent) => {
      if (!open) return;
      e.preventDefault();
      e.stopPropagation();
      setIsDragging(true);
      const startX = e.clientX;
      const startW = widthRef.current;

      const handleMove = (ev: MouseEvent) => {
        // Left-edge handle: moving left grows the panel.
        const delta = startX - ev.clientX;
        onResize(clampResizableRightPanelWidth(startW + delta));
      };
      const handleUp = () => {
        setIsDragging(false);
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
        document.removeEventListener('mousemove', handleMove);
        document.removeEventListener('mouseup', handleUp);
      };

      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      document.addEventListener('mousemove', handleMove);
      document.addEventListener('mouseup', handleUp);
    },
    [open, onResize],
  );

  const handleDragDoubleClick = useCallback(() => {
    onResize(clampResizableRightPanelWidth(RESIZABLE_RIGHT_PANEL_DEFAULT_WIDTH));
  }, [onResize]);

  if (!open) return null;

  const density = width < 300 ? 'compact' : width >= 480 ? 'wide' : 'normal';
  const regionLabel = ariaLabel || title || 'Right panel';

  return (
    <aside
      className={`relative flex h-full min-h-0 shrink-0 flex-col border-l border-[var(--border)] bg-[var(--surface)] ${className}`}
      role="region"
      aria-label={regionLabel}
      data-density={density}
      data-resizing={isDragging ? 'true' : 'false'}
      style={{
        width,
        transition: isDragging ? 'none' : undefined,
      }}
    >
      <div
        className={`absolute inset-y-0 left-0 z-10 w-1 cursor-col-resize hover:bg-[var(--primary)]/30 ${
          isDragging ? 'bg-[var(--primary)]/40' : ''
        }`}
        onMouseDown={handleDragStart}
        onDoubleClick={handleDragDoubleClick}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={width}
        aria-valuemin={RESIZABLE_RIGHT_PANEL_MIN_WIDTH}
        aria-valuemax={rightPanelViewportMax()}
        aria-label={resizeLabel}
        title={resizeHint}
      />

      <div className="flex h-11 shrink-0 items-center gap-1 border-b border-[var(--border)] px-2">
        {tabs && tabs.length > 0 ? (
          <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto">
            {tabs.map((tab) => {
              const active = tab.id === activeTabId;
              return (
                <button
                  key={tab.id}
                  type="button"
                  onClick={() => onTabChange?.(tab.id)}
                  title={tab.title || tab.label}
                  className={`flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors ${
                    active
                      ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                      : 'text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
                  }`}
                >
                  {tab.icon}
                  <span className="truncate">{tab.label}</span>
                </button>
              );
            })}
          </div>
        ) : (
          <div className="min-w-0 flex-1 truncate px-1 text-sm font-medium text-[var(--text)]">
            {title}
          </div>
        )}
        {headerExtra}
        {onClose && (
          <button
            type="button"
            onClick={onClose}
            className="ml-auto flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
            title={closeLabel}
            aria-label={closeLabel}
          >
            <X size={14} />
          </button>
        )}
      </div>

      <div className={`min-h-0 flex-1 ${scrollBody ? 'overflow-y-auto' : 'overflow-hidden'}`}>
        {children}
      </div>
    </aside>
  );
}
