'use client';

/**
 * WorkspaceCompositionPage (C-001..C-006) — the V2 personal workspace surface.
 *
 *  - snapshot-first: session state hydrates synchronously from the cache, so
 *    the first paint is never blank.
 *  - tab strip: open / close / pin / reorder; Close != Delete (recently closed
 *    list can reopen a tab; the view config stays in the snapshot).
 *  - inactive tabs carry only metadata + snapshot cache; only the active view
 *    is mounted.
 *  - inspector host (ResizableRightPanel-based) on the right.
 *
 * Shared-surface wiring (mounting this inside ShellLayout/MainContent) is
 * described as a patch intent in docs/development/handoff-c.md — the shared
 * files themselves are out of scope here.
 */

import { useCallback, useRef, useState } from 'react';
import { Eye, EyeOff, PanelRight, Save } from 'lucide-react';
import {
  WorkspaceSessionProvider,
  useWorkspaceSession,
} from './session/WorkspaceSessionProvider';
import WorkspaceTabStrip from './tabs/WorkspaceTabStrip';
import WorkspaceInspector, { type CanvasInspectorAction } from './inspector/WorkspaceInspector';
import GridWorkspaceView from './views/GridWorkspaceView';
import FreeCanvasView, { type FreeCanvasViewHandle } from './views/FreeCanvasView';
import DataView, { type DataRow } from './views/DataView';
import { createWorkspaceViewConfig, emptyGridLayouts } from './views/WorkspaceViewRegistry';
import type { CanvasNode } from '@/lib/workspace/canvas/types';
import type { DataViewState, WorkspaceViewKind } from '@/lib/workspace/views/types';

export default function WorkspaceCompositionPage() {
  return (
    <WorkspaceSessionProvider>
      <WorkspaceCompositionInner />
    </WorkspaceSessionProvider>
  );
}

function WorkspaceCompositionInner() {
  const { snapshot, api } = useWorkspaceSession();
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [inspectorWidth, setInspectorWidth] = useState(300);
  const [gridEditing, setGridEditing] = useState(false);
  const [canvasSelectionIds, setCanvasSelectionIds] = useState<string[]>([]);
  const canvasRef = useRef<FreeCanvasViewHandle | null>(null);

  const { tabs, session, views } = snapshot;
  const activeTabId = session.activeTabId;
  const activeView = activeTabId ? views[activeTabId] ?? null : null;

  const handleNewView = useCallback(
    (kind: WorkspaceViewKind) => {
      api.addView(createWorkspaceViewConfig(kind, tabs.length));
      setGridEditing(false);
    },
    [api, tabs.length],
  );

  const handleCanvasAction = useCallback(
    (action: CanvasInspectorAction) => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      switch (action.type) {
        case 'delete':
          canvas.deleteSelection();
          break;
        case 'group':
          canvas.groupSelection();
          break;
        case 'ungroup':
          canvas.ungroupSelection();
          break;
        case 'front':
          canvas.bringToFront();
          break;
        case 'back':
          canvas.sendToBack();
          break;
      }
    },
    [],
  );

  const handleGridRemoveItem = useCallback(
    (itemId: string) => {
      if (!activeView || activeView.kind !== 'grid' || !activeView.gridLayouts) return;
      const layouts = activeView.gridLayouts;
      const next = {
        lg: layouts.lg.filter((item) => item.i !== itemId),
        md: layouts.md.filter((item) => item.i !== itemId),
        sm: layouts.sm.filter((item) => item.i !== itemId),
      };
      api.updateView(activeView.id, { gridLayouts: next });
    },
    [activeView, api],
  );

  const dataState: DataViewState = activeView?.kind === 'data' && activeView.data
    ? activeView.data
    : { mode: 'list', columns: [], hiddenColumns: [], filters: [], sort: null, groupBy: null, calendarField: undefined };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--surface-subtle)]" data-workspace-v2>
      <WorkspaceHeader
        name={snapshot.name}
        activeKind={activeView?.kind ?? null}
        gridEditing={gridEditing}
        onToggleGridEditing={activeView?.kind === 'grid' ? setGridEditing : undefined}
        inspectorOpen={inspectorOpen}
        onToggleInspector={() => setInspectorOpen((open) => !open)}
      />

      <WorkspaceTabStrip
        tabs={tabs}
        activeTabId={activeTabId}
        closedTabs={session.closedTabs}
        onActivate={api.activate}
        onClose={api.closeTab}
        onReopen={api.reopenTab}
        onPin={api.pinTab}
        onReorder={api.reorderTab}
        onNewView={handleNewView}
      />

      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">
          {activeView ? (
            <ViewBody
              view={activeView}
              gridEditing={gridEditing}
              onGridLayoutChange={(layouts) => api.updateView(activeView.id, { gridLayouts: layouts })}
              onGridBreakpointChange={(bp) => api.setBreakpoint(bp)}
              onGridRemoveItem={handleGridRemoveItem}
              onDataStateChange={(patch) =>
                api.updateView(activeView.id, {
                  data: { ...dataState, ...patch },
                })
              }
              canvasRef={canvasRef}
              onCanvasCommit={(nodes) => api.updateView(activeView.id, { canvasNodes: nodes })}
              onCanvasSelectionChange={setCanvasSelectionIds}
            />
          ) : (
            <EmptyWorkspace onNewView={handleNewView} />
          )}
        </div>

        <WorkspaceInspector
          open={inspectorOpen}
          width={inspectorWidth}
          onResize={setInspectorWidth}
          onClose={() => setInspectorOpen(false)}
          activeView={activeView}
          canvasSelectionIds={canvasSelectionIds}
          onCanvasAction={handleCanvasAction}
          onEditGrid={setGridEditing}
          gridEditing={gridEditing}
        />
      </div>

      <StatusBar
        savedHint="Saved to snapshot cache"
        viewCount={tabs.length}
        activeKind={activeView?.kind ?? '—'}
      />
    </div>
  );
}

function WorkspaceHeader({
  name,
  activeKind,
  gridEditing,
  onToggleGridEditing,
  inspectorOpen,
  onToggleInspector,
}: {
  name: string;
  activeKind: WorkspaceViewKind | null;
  gridEditing: boolean;
  onToggleGridEditing?: (editing: boolean) => void;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
}) {
  return (
    <div className="flex h-11 shrink-0 items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-3">
      <span className="truncate text-sm font-medium text-[var(--text)]">{name}</span>
      {activeKind && (
        <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-[0.625rem] uppercase tracking-wide text-[var(--text-disabled)]">
          {activeKind}
        </span>
      )}
      <div className="ml-auto flex items-center gap-1">
        {onToggleGridEditing && (
          <button
            type="button"
            onClick={() => onToggleGridEditing(!gridEditing)}
            aria-pressed={gridEditing}
            className={`inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs transition-colors ${
              gridEditing
                ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
            }`}
          >
            {gridEditing ? <EyeOff size={13} /> : <Eye size={13} />}
            {gridEditing ? 'Done editing' : 'Edit layout'}
          </button>
        )}
        <button
          type="button"
          onClick={onToggleInspector}
          aria-pressed={inspectorOpen}
          title="Toggle inspector"
          className={`inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs transition-colors ${
            inspectorOpen
              ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
              : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
          }`}
        >
          <PanelRight size={13} />
          Inspector
        </button>
      </div>
    </div>
  );
}

function ViewBody({
  view,
  gridEditing,
  onGridLayoutChange,
  onGridBreakpointChange,
  onGridRemoveItem,
  onDataStateChange,
  canvasRef,
  onCanvasCommit,
  onCanvasSelectionChange,
}: {
  view: { id: string; kind: WorkspaceViewKind; title: string; gridLayouts?: unknown; canvasNodes?: unknown; data?: DataViewState };
  gridEditing: boolean;
  onGridLayoutChange: (layouts: import('@/lib/workspace/views/types').GridLayouts) => void;
  onGridBreakpointChange: (bp: 'lg' | 'md' | 'sm') => void;
  onGridRemoveItem: (id: string) => void;
  onDataStateChange: (patch: Partial<DataViewState>) => void;
  canvasRef: React.Ref<FreeCanvasViewHandle>;
  onCanvasCommit: (nodes: CanvasNode[]) => void;
  onCanvasSelectionChange: (ids: string[]) => void;
}) {
  switch (view.kind) {
    case 'grid':
      return (
        <GridWorkspaceView
          key={view.id}
          viewId={view.id}
          layouts={(view.gridLayouts as import('@/lib/workspace/views/types').GridLayouts) ?? emptyGridLayouts()}
          editable={gridEditing}
          onLayoutChange={onGridLayoutChange}
          onBreakpointChange={onGridBreakpointChange}
          onRemoveItem={onGridRemoveItem}
        />
      );
    case 'canvas':
      return (
        <FreeCanvasView
          key={view.id}
          ref={canvasRef}
          initialNodes={(view.canvasNodes as CanvasNode[]) ?? []}
          onCommit={onCanvasCommit}
          onSelectionChange={onCanvasSelectionChange}
          editable
        />
      );
    case 'data':
      return (
        <DataView
          key={view.id}
          viewId={view.id}
          state={view.data ?? { mode: 'list', columns: [], hiddenColumns: [], filters: [], sort: null, groupBy: null, calendarField: undefined }}
          onStateChange={onDataStateChange}
          rows={demoRows}
        />
      );
    default:
      return null;
  }
}

function EmptyWorkspace({ onNewView }: { onNewView: (kind: WorkspaceViewKind) => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
      <div className="text-sm font-medium text-[var(--text)]">No view is open</div>
      <p className="max-w-80 text-xs leading-relaxed text-[var(--text-secondary)]">
        Create a view to start. Closed views are kept in the snapshot and can be
        reopened from the tab strip history.
      </p>
      <div className="flex gap-2">
        {(['grid', 'canvas', 'data'] as WorkspaceViewKind[]).map((kind) => (
          <button
            key={kind}
            type="button"
            onClick={() => onNewView(kind)}
            className="rounded-lg bg-[var(--primary)] px-3 py-2 text-xs font-medium text-[var(--primary-foreground)] transition-colors hover:opacity-90"
          >
            New {kind} view
          </button>
        ))}
      </div>
    </div>
  );
}

function StatusBar({
  savedHint,
  viewCount,
  activeKind,
}: {
  savedHint: string;
  viewCount: number;
  activeKind: string;
}) {
  return (
    <div className="flex h-6 shrink-0 items-center gap-3 border-t border-[var(--border)] bg-[var(--surface)] px-3 text-[0.625rem] text-[var(--text-disabled)]">
      <span className="inline-flex items-center gap-1">
        <Save size={11} />
        {savedHint}
      </span>
      <span className="tabular-nums">{viewCount} view(s)</span>
      <span className="ml-auto uppercase tracking-wide">{activeKind}</span>
    </div>
  );
}

/** Sample rows for the Data view (Wave1 demo; rows move to the real source in Wave2). */
const demoRows: DataRow[] = [
  { id: 'd1', name: 'Draft V2 workspace spec', status: 'todo', assignee: 'You', due: '2026-01-12' },
  { id: 'd2', name: 'Wire CompactGrid keyboard layer', status: 'in-progress', assignee: 'You', due: '2026-01-14' },
  { id: 'd3', name: 'Free Canvas marquee select', status: 'done', assignee: 'B', due: '2026-01-08' },
  { id: 'd4', name: 'Inspector host generalization', status: 'in-progress', assignee: 'C', due: '2026-01-15' },
  { id: 'd5', name: 'Snapshot-first hydration', status: 'done', assignee: 'C', due: '2026-01-06' },
  { id: 'd6', name: 'Data view calendar field', status: 'todo', assignee: 'A', due: '2026-01-20' },
];
