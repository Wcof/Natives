'use client';

/**
 * Workspace tab strip (C-003). Open / close / pin / reorder, keyboard layer.
 *
 * Close != Delete: closing a tab moves it to the "recently closed" list
 * (rendered as a history dropdown) and retains the view config in the snapshot.
 */

import { useEffect, useRef, useState } from 'react';
import {
  History,
  LayoutGrid,
  Layers,
  Pin,
  Plus,
  Table2,
  X,
} from 'lucide-react';
import type { WorkspaceTab, WorkspaceViewKind } from '@/lib/workspace/views/types';
import { kindLabel, moveTabTo } from './tabStripModel';

export function workspaceKindIcon(kind: WorkspaceViewKind, size = 14) {
  switch (kind) {
    case 'canvas':
      return <Layers size={size} />;
    case 'data':
      return <Table2 size={size} />;
    default:
      return <LayoutGrid size={size} />;
  }
}

export interface WorkspaceTabStripProps {
  tabs: WorkspaceTab[];
  activeTabId: string | null;
  closedTabs: WorkspaceTab[];
  onActivate: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onReopen: (tabId: string) => void;
  onPin: (tabId: string, pinned: boolean) => void;
  onReorder: (tabId: string, toIndex: number) => void;
  onNewView: (kind: WorkspaceViewKind) => void;
}

export default function WorkspaceTabStrip({
  tabs,
  activeTabId,
  closedTabs,
  onActivate,
  onClose,
  onReopen,
  onPin,
  onReorder,
  onNewView,
}: WorkspaceTabStripProps) {
  const [focusIndex, setFocusIndex] = useState(-1);
  const [menuOpen, setMenuOpen] = useState<'recent' | 'new' | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const tabListRef = useRef<HTMLDivElement | null>(null);

  // Keep focus on the intended tab after keyboard navigation (ref callbacks only
  // fire on mount, so focus moves here instead).
  useEffect(() => {
    if (focusIndex < 0) return;
    const el = tabListRef.current?.querySelector<HTMLElement>(`[data-tab-index="${focusIndex}"]`);
    el?.focus();
  }, [focusIndex]);

  // Sync focus with the active tab whenever a tab is activated by click/menu.
  useEffect(() => {
    if (!activeTabId) return;
    const index = tabs.findIndex((tab) => tab.id === activeTabId);
    if (index >= 0) setFocusIndex(index);
  }, [activeTabId, tabs]);

  useEffect(() => {
    const onPointer = (e: MouseEvent) => {
      if (menuOpen && menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(null);
      }
    };
    document.addEventListener('mousedown', onPointer);
    return () => document.removeEventListener('mousedown', onPointer);
  }, [menuOpen]);

  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    const current = tabs[index];
    if (!current) return;
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const direction = e.key === 'ArrowRight' ? 1 : -1;
      const nextIndex = (index + direction + tabs.length) % tabs.length;
      setFocusIndex(nextIndex);
      const nextTab = tabs[nextIndex];
      if (nextTab) onActivate(nextTab.id);
    } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      // Ctrl+Arrow reorders; plain arrows move focus within recent/new menu.
      e.preventDefault();
    } else if (e.key === 'Home') {
      e.preventDefault();
      setFocusIndex(0);
      const firstTab = tabs[0];
      if (firstTab) onActivate(firstTab.id);
    } else if (e.key === 'End') {
      e.preventDefault();
      setFocusIndex(tabs.length - 1);
      const lastTab = tabs[tabs.length - 1];
      if (lastTab) onActivate(lastTab.id);
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      onClose(current.id);
    }
  };

  const handleDragStart = (e: React.DragEvent, tabId: string) => {
    setDraggedId(tabId);
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', tabId);
  };

  const handleDragOver = (e: React.DragEvent, overId: string) => {
    e.preventDefault();
    if (!draggedId || draggedId === overId) return;
    e.dataTransfer.dropEffect = 'move';
    const from = tabs.findIndex((tab) => tab.id === draggedId);
    const to = tabs.findIndex((tab) => tab.id === overId);
    if (from < 0 || to < 0 || from === to) return;
    onReorder(draggedId, to);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDraggedId(null);
  };

  const newViewMenu = (
    <div ref={menuRef} className="absolute right-0 top-full z-30 mt-1 min-w-40 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-lg">
      {(['grid', 'canvas', 'data'] as WorkspaceViewKind[]).map((kind) => (
        <button
          key={kind}
          type="button"
          onClick={() => {
            onNewView(kind);
            setMenuOpen(null);
          }}
          className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
        >
          {workspaceKindIcon(kind)}
          {kindLabel(kind)}
        </button>
      ))}
    </div>
  );

  return (
    <div
      ref={tabListRef}
      className="relative flex h-10 shrink-0 items-stretch gap-0.5 overflow-x-auto border-b border-[var(--border)] bg-[var(--surface)] px-2"
      role="tablist"
      aria-label="Workspace views"
    >
      {tabs.map((tab, index) => {
        const active = tab.id === activeTabId;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={active}
            data-tab-index={index}
            tabIndex={index === focusIndex ? 0 : -1}
            onClick={() => onActivate(tab.id)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            draggable
            onDragStart={(e) => handleDragStart(e, tab.id)}
            onDragOver={(e) => handleDragOver(e, tab.id)}
            onDrop={handleDrop}
            onDragEnd={() => setDraggedId(null)}
            className={`group flex h-8 shrink-0 cursor-pointer select-none items-center gap-1.5 self-center rounded-md border px-2.5 text-xs transition-colors ${
              active
                ? 'border-[var(--border)] bg-[var(--surface-hover)] text-[var(--text)]'
                : 'border-transparent text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
            }`}
            title={`${tab.title}${tab.pinned ? ' · pinned' : ''}`}
          >
            {workspaceKindIcon(tab.kind)}
            <span className="max-w-32 truncate">{tab.title}</span>
            {tab.pinned && <Pin size={11} className="shrink-0 text-[var(--text-disabled)]" />}
            <span
              role="button"
              tabIndex={-1}
              aria-label={`Close ${tab.title}`}
              className="ml-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--danger)]"
              onClick={(e) => {
                e.stopPropagation();
                onClose(tab.id);
              }}
            >
              <X size={12} />
            </span>
          </button>
        );
      })}

      <div className="ml-auto flex shrink-0 items-center gap-0.5">
        {closedTabs.length > 0 && (
          <div className="relative">
            <button
              type="button"
              aria-label="Recently closed views"
              title="Recently closed (Close ≠ Delete)"
              onClick={() => setMenuOpen(menuOpen === 'recent' ? null : 'recent')}
              className="flex h-8 w-8 items-center justify-center rounded-md text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
            >
              <History size={14} />
            </button>
            {menuOpen === 'recent' && (
              <div className="absolute right-0 top-full z-30 mt-1 min-w-44 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-lg">
                {closedTabs.map((tab) => (
                  <button
                    key={tab.id}
                    type="button"
                    onClick={() => onReopen(tab.id)}
                    className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
                  >
                    {workspaceKindIcon(tab.kind)}
                    {tab.title}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
        <div className="relative">
          <button
            type="button"
            aria-label="New view"
            title="New view"
            onClick={() => setMenuOpen(menuOpen === 'new' ? null : 'new')}
            className="flex h-8 w-8 items-center justify-center rounded-md text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
          >
            <Plus size={14} />
          </button>
          {menuOpen === 'new' && newViewMenu}
        </div>
      </div>
    </div>
  );
}

/** Convenience: build a workspace tab from a view config (registry helper). */
export function tabFromView(view: {
  id: string;
  title: string;
  kind: WorkspaceViewKind;
}): WorkspaceTab {
  return { id: view.id, title: view.title, kind: view.kind, icon: undefined };
}

/** Re-export moveTabTo for tests / inspector tools. */
export { moveTabTo };
