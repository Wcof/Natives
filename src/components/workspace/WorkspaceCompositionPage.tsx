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

import { useCallback, useRef, useState, lazy, Suspense } from 'react';
import { useLocale, t } from '@/i18n';
import { Eye, EyeOff, PanelRight, X } from 'lucide-react';
import { ErrorPrimitive, Skeleton } from '@/components/ui/design-system';
import {
  WorkspaceSessionProvider,
  useWorkspaceSession,
} from './session/WorkspaceSessionProvider';
import WorkspaceTabStrip from './tabs/WorkspaceTabStrip';
import { kindLabel } from './tabs/tabStripModel';
import type { CanvasInspectorAction } from './inspector/WorkspaceInspector';
import type { FreeCanvasViewHandle } from './views/FreeCanvasView';
import { createWorkspaceViewConfig, emptyGridLayouts } from './views/WorkspaceViewRegistry';
import type { CanvasNode } from '@/lib/workspace/canvas/types';
import type { DataViewState, WorkspaceViewKind } from '@/lib/workspace/views/types';

const GridWorkspaceView = lazy(() => import('./views/GridWorkspaceView'));
const FreeCanvasView = lazy(() => import('./views/FreeCanvasView'));
const DataView = lazy(() => import('./views/DataView'));
const WorkspaceInspector = lazy(() => import('./inspector/WorkspaceInspector'));

export default function WorkspaceCompositionPage() {
  return (
    <WorkspaceSessionProvider>
      <WorkspaceCompositionInner />
    </WorkspaceSessionProvider>
  );
}

function WorkspaceCompositionInner() {
  const locale = useLocale();
  const { snapshot, api, workspaceId, hostStatus, hostError, reloadHost } = useWorkspaceSession();
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [inspectorWidth, setInspectorWidth] = useState(300);
  const [gridEditing, setGridEditing] = useState(false);
  const [canvasSelectionIds, setCanvasSelectionIds] = useState<string[]>([]);
  /** Real error text from a failed grid host write (Slice 14) — never silently dropped. */
  const [gridWriteError, setGridWriteError] = useState<string | null>(null);
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

  /**
   * Terminal layout STOP → host persistence (Slice 14 pointer rule).
   * The grid invokes this ONLY from RGL onDragStop/onResizeStop — never from
   * pointer move. The provider's saveLayout writes the host layout rows
   * (per breakpoint) and re-fetches the snapshot so the grid re-renders from
   * the Host. The promise is returned so a real write error (e.g. G-008
   * `Conflict`) surfaces instead of being silently dropped.
   */
  const handleGridLayoutChange = useCallback(
    (
      layouts: import('@/lib/workspace/views/types').GridLayouts,
      breakpoint: 'lg' | 'md' | 'sm',
    ) => api.saveLayout(layouts, breakpoint),
    [api],
  );

  const handleGridWriteError = useCallback((message: string) => {
    setGridWriteError(message);
  }, []);

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

      {gridWriteError && (
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--danger)]/30 bg-[var(--danger-soft)] px-3 py-1.5 text-xs text-[var(--danger)]">
          <span className="min-w-0 flex-1 truncate">{gridWriteError}</span>
          <button
            type="button"
            onClick={() => setGridWriteError(null)}
            title={t(locale, 'common.close')}
            aria-label={t(locale, 'common.close')}
            className="shrink-0 rounded p-0.5 transition-colors hover:bg-[var(--surface-hover)] focus-visible:outline-2 focus-visible:outline-[var(--primary)]"
          >
            <X size={12} />
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">
          {hostStatus === 'error' ? (
            <div className="flex h-full items-center justify-center overflow-auto">
              <ErrorPrimitive
                message={hostError ?? t(locale, 'common.error')}
                onRetry={reloadHost}
                retryLabel={t(locale, 'common.retry')}
              />
            </div>
          ) : hostStatus === 'pending' ? (
            <div className="flex h-full flex-col gap-3 p-4">
              <Skeleton variant="text" lines={1} width="30%" />
              <div className="grid grid-cols-2 gap-3">
                <Skeleton variant="card" lines={3} />
                <Skeleton variant="card" lines={3} />
                <Skeleton variant="card" lines={3} />
                <Skeleton variant="card" lines={3} />
              </div>
            </div>
          ) : activeView ? (
            <Suspense fallback={<div className="flex h-full items-center justify-center p-8"><Skeleton variant="card" lines={3} /></div>}>
              <ViewBody
                view={activeView}
                workspaceId={workspaceId}
                gridEditing={gridEditing}
                onGridLayoutChange={handleGridLayoutChange}
                onGridBreakpointChange={(bp) => api.setBreakpoint(bp)}
                onGridAddWidget={api.addWidget}
                onGridRemoveWidget={api.removeWidget}
                onGridWriteError={handleGridWriteError}
                onDataStateChange={(patch) =>
                  api.updateView(activeView.id, {
                    data: { ...dataState, ...patch },
                  })
                }
                canvasRef={canvasRef}
                onCanvasCommit={(nodes) => api.updateView(activeView.id, { canvasNodes: nodes })}
                onCanvasSelectionChange={setCanvasSelectionIds}
              />
            </Suspense>
          ) : (
            <EmptyWorkspace onNewView={handleNewView} />
          )}
        </div>

        <Suspense fallback={null}>
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
            onDataStateChange={
              activeView?.kind === 'data'
                ? (patch) =>
                    api.updateView(activeView.id, {
                      data: { ...dataState, ...patch },
                    })
                : undefined
            }
            onRenameView={
              activeView
                ? (title) => api.updateView(activeView.id, { title })
                : undefined
            }
          />
        </Suspense>
      </div>

      <StatusBar
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
  const locale = useLocale();
  return (
    <div className="flex h-11 shrink-0 items-center gap-2 border-b border-[var(--border)] bg-[var(--surface)] px-3">
      <span className="truncate text-sm font-medium text-[var(--text)]">{name === "Workspace" || name === "Default Workspace" ? t(locale, "workspace.defaultWorkspaceName") : name}</span>
      {activeKind && (
        <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-[0.625rem] uppercase tracking-wide text-[var(--text-disabled)]">
          {kindLabel(activeKind, locale)}
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
            {gridEditing ? t(locale, 'workspace.doneEditing') : t(locale, 'workspace.editLayout')}
          </button>
        )}
        <button
          type="button"
          onClick={onToggleInspector}
          aria-pressed={inspectorOpen}
          title={t(locale, 'workspace.toggleInspector')}
          className={`inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs transition-colors ${
            inspectorOpen
              ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
              : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
          }`}
        >
          <PanelRight size={13} />
          {t(locale, 'workspace.inspector')}
        </button>
      </div>
    </div>
  );
}

function ViewBody({
  view,
  workspaceId,
  gridEditing,
  onGridLayoutChange,
  onGridBreakpointChange,
  onGridAddWidget,
  onGridRemoveWidget,
  onGridWriteError,
  onDataStateChange,
  canvasRef,
  onCanvasCommit,
  onCanvasSelectionChange,
}: {
  view: { id: string; kind: WorkspaceViewKind; title: string; gridLayouts?: unknown; canvasNodes?: unknown; data?: DataViewState };
  workspaceId: string | null;
  gridEditing: boolean;
  /** Terminal drag/resize STOP only (never pointer move) → host saveLayout. */
  onGridLayoutChange: (
    layouts: import('@/lib/workspace/views/types').GridLayouts,
    breakpoint: 'lg' | 'md' | 'sm',
  ) => Promise<void>;
  onGridBreakpointChange: (bp: 'lg' | 'md' | 'sm') => void;
  /** Host write surface (Slice 14): upsert / remove widget rows via the provider. */
  onGridAddWidget: (widgetType: string) => Promise<void>;
  onGridRemoveWidget: (widgetId: string) => Promise<void>;
  onGridWriteError: (message: string) => void;
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
          workspaceId={workspaceId}
          viewId={view.id}
          layouts={(view.gridLayouts as import('@/lib/workspace/views/types').GridLayouts) ?? emptyGridLayouts()}
          editable={gridEditing}
          onLayoutChange={onGridLayoutChange}
          onBreakpointChange={onGridBreakpointChange}
          onAddWidget={onGridAddWidget}
          onRemoveWidget={onGridRemoveWidget}
          onWriteError={onGridWriteError}
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
        />
      );
    default:
      return null;
  }
}

function EmptyWorkspace({ onNewView }: { onNewView: (kind: WorkspaceViewKind) => void }) {
  const locale = useLocale();
  const newViewKey: Record<WorkspaceViewKind, string> = {
    grid: 'workspace.newGridView',
    canvas: 'workspace.newCanvasView',
    data: 'workspace.newDataView',
  };
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-8 text-center">
      <div className="text-sm font-medium text-[var(--text)]">{t(locale, 'workspace.noOpenViews')}</div>
      <p className="max-w-80 text-xs leading-relaxed text-[var(--text-secondary)]">
        {t(locale, 'workspace.createViewHint')}
      </p>
      <div className="flex gap-2">
        {(['grid', 'canvas', 'data'] as WorkspaceViewKind[]).map((kind) => (
          <button
            key={kind}
            type="button"
            onClick={() => onNewView(kind)}
            className="rounded-lg bg-[var(--primary)] px-3 py-2 text-xs font-medium text-[var(--primary-foreground)] transition-colors hover:opacity-90"
          >
            {t(locale, newViewKey[kind])}
          </button>
        ))}
      </div>
    </div>
  );
}

function StatusBar({
  viewCount,
  activeKind,
}: {

  viewCount: number;
  activeKind: string;
}) {
  const locale = useLocale();
  return (
    <div className="flex h-6 shrink-0 items-center gap-3 border-t border-[var(--border)] bg-[var(--surface)] px-3 text-[0.625rem] text-[var(--text-disabled)]">
      <span className="tabular-nums">{t(locale, 'workspace.viewCount', { count: viewCount })}</span>
      <span className="ml-auto uppercase tracking-wide">
        {activeKind === '—' ? '—' : kindLabel(activeKind as WorkspaceViewKind, locale)}
      </span>
    </div>
  );
}

/** Sample rows for the Data view (Wave1 demo; rows move to the real source in Wave2). */

