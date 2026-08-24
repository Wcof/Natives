'use client';

import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, LayoutDashboard, PanelRight, Plus, Redo2, RotateCcw, Save, Settings2, Undo2, X } from 'lucide-react';
import { ErrorPrimitive, Skeleton } from '@/components/ui/design-system';
import { t, useLocale } from '@/i18n';
import type { CanvasNode } from '@/lib/workspace/canvas/types';
import type { WorkspaceLayoutMode } from '@/lib/workspace/contracts';
import type { GridLayouts } from '@/lib/workspace/views/types';
import { TimeRangeContext } from '@/lib/workspace/widgets/time-range-context';
import type { TimeRange } from '@/lib/workspace/widgets/types';
import { AddWidgetMenu } from './widgets/AddWidgetMenu';
import { getWidget } from '@/lib/workspace/widgets';
import { WorkspaceSessionProvider, useWorkspaceSession } from './session/WorkspaceSessionProvider';
import '@/app/styles/widgets.css';

const GridWorkspaceView = lazy(() => import('./views/GridWorkspaceView'));
const FreeCanvasView = lazy(() => import('./views/FreeCanvasView'));

const EMPTY_LAYOUTS: GridLayouts = { lg: [], md: [], sm: [] };

export default function WorkspaceCompositionPage() {
  return <WorkspaceSessionProvider><WorkspaceDashboard /></WorkspaceSessionProvider>;
}

function WorkspaceDashboard() {
  const locale = useLocale();
  const { session, snapshot, templates, status, error, editing, api } = useWorkspaceSession();
  const [timeRange, setTimeRange] = useState<TimeRange>('7d');
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [selectedWidgetId, setSelectedWidgetId] = useState<string | null>(null);
  const [writeError, setWriteError] = useState<string | null>(null);
  const [undo, setUndo] = useState<GridLayouts[]>([]);
  const [redo, setRedo] = useState<GridLayouts[]>([]);
  const [draggedWorkspaceId, setDraggedWorkspaceId] = useState<string | null>(null);
  const addButtonRef = useRef<HTMLButtonElement | null>(null);

  const layouts = useMemo<GridLayouts>(() => {
    if (!snapshot) return EMPTY_LAYOUTS;
    const next: GridLayouts = { lg: [], md: [], sm: [] };
    for (const row of snapshot.layouts) {
      if (row.layoutMode === 'structured' && row.breakpoint !== 'free' && Array.isArray(row.layout)) next[row.breakpoint] = row.layout as GridLayouts['lg'];
    }
    return next;
  }, [snapshot]);

  const freeNodes = useMemo<CanvasNode[]>(() => {
    if (!snapshot) return [];
    const stored = snapshot.layouts.find((row) => row.layoutMode === 'free' || row.breakpoint === 'free')?.layout;
    if (Array.isArray(stored)) return stored as CanvasNode[];
    if (stored && typeof stored === 'object' && Array.isArray((stored as { nodes?: unknown }).nodes)) return (stored as { nodes: CanvasNode[] }).nodes;
    return snapshot.widgets.filter((widget) => widget.enabled).map((widget, index) => ({ id: widget.id, kind: 'widget', label: widget.widgetType, widgetType: widget.widgetType, widgetConfig: widget.config, x: 40 + (index % 3) * 280, y: 40 + Math.floor(index / 3) * 210, w: 256, h: 184, z: widget.zIndex }));
  }, [snapshot]);

  const commitGrid = useCallback(async (next: GridLayouts, breakpoint: 'lg' | 'md' | 'sm') => {
    setUndo((history) => [...history.slice(-29), layouts]); setRedo([]);
    try { await api.saveStructuredLayout(next, breakpoint); } catch (cause) { setWriteError(String(cause)); }
  }, [api, layouts]);

  const applyHistory = useCallback(async (direction: 'undo' | 'redo') => {
    const source = direction === 'undo' ? undo : redo;
    const target = source[source.length - 1];
    if (!target) return;
    if (direction === 'undo') { setUndo(source.slice(0, -1)); setRedo((items) => [...items, layouts]); }
    else { setRedo(source.slice(0, -1)); setUndo((items) => [...items, layouts]); }
    for (const breakpoint of ['lg', 'md', 'sm'] as const) await api.saveStructuredLayout(target, breakpoint);
  }, [api, layouts, redo, undo]);

  useEffect(() => {
    if (!editing) return;
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches('input,textarea,[contenteditable=true]')) return;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); void applyHistory(event.shiftKey ? 'redo' : 'undo');
      }
      if ((event.key === 'Delete' || event.key === 'Backspace') && selectedWidgetId) {
        event.preventDefault(); void api.removeWidget(selectedWidgetId).then(() => setSelectedWidgetId(null));
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [api, applyHistory, editing, selectedWidgetId]);

  if (status === 'error') return <div className="flex h-full items-center justify-center"><ErrorPrimitive message={error ?? t(locale, 'common.error')} onRetry={api.reload} retryLabel={t(locale, 'common.retry')} /></div>;
  if (status === 'pending' || !snapshot) return <DashboardSkeleton />;

  const mode = snapshot.workspace.defaultLayoutMode;
  const openTabs = session?.openedTabs ?? [];

  return (
    <TimeRangeContext.Provider value={timeRange}>
      <main className="workspace-dashboard flex h-full min-h-0 flex-col overflow-hidden bg-[var(--surface-subtle)]" data-mode={editing ? 'edit' : 'browse'} data-layout={mode}>
        <nav className="workspace-session-tabs flex h-10 shrink-0 items-center gap-1 border-b border-[var(--border-subtle)] px-3" aria-label="Workspace sessions">
          {openTabs.map((tab) => {
            const item = session?.workspaces.find((workspace) => workspace.id === tab.workspaceId);
            const active = tab.workspaceId === snapshot.workspace.id;
            return <button key={tab.workspaceId} type="button" draggable onDragStart={() => setDraggedWorkspaceId(tab.workspaceId)} onDragOver={(event) => event.preventDefault()} onDrop={() => { if (!draggedWorkspaceId || draggedWorkspaceId === tab.workspaceId) return; const ids = openTabs.map((entry) => entry.workspaceId); const from = ids.indexOf(draggedWorkspaceId); const to = ids.indexOf(tab.workspaceId); ids.splice(to, 0, ids.splice(from, 1)[0]!); void api.reorderWorkspaces(ids); setDraggedWorkspaceId(null); }} onClick={() => void api.openWorkspace(tab.workspaceId)} className={`group inline-flex h-8 max-w-52 items-center gap-2 rounded-lg px-3 text-xs ${active ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm' : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'}`} aria-current={active ? 'page' : undefined}><span className="truncate">{item?.name ?? tab.workspaceId}</span>{openTabs.length > 1 && <X size={12} className="opacity-0 group-hover:opacity-70" onClick={(event) => { event.stopPropagation(); void api.closeWorkspace(tab.workspaceId); }} />}</button>;
          })}
          <button type="button" className="ws-dashboard-action shrink-0" onClick={() => void api.createWorkspace()} aria-label={t(locale, 'workspace.newWorkspace')}><Plus size={14} /></button>
        </nav>

        <header className="workspace-dashboard-header flex shrink-0 items-center gap-3 px-6 pb-3 pt-5">
          <div className="min-w-0">
            <p className="text-[0.6875rem] uppercase tracking-[0.16em] text-[var(--text-disabled)]">{t(locale, 'workspace.dashboardSubtitle')}</p>
            <h1 className="truncate text-xl font-semibold text-[var(--text)]">{snapshot.workspace.name}</h1>
          </div>
          <div className="ml-auto flex items-center gap-1.5">
            {!editing && <TimeRangePicker value={timeRange} onChange={setTimeRange} />}
            {editing && <>
              <button className="ws-dashboard-action" disabled={!undo.length} onClick={() => void applyHistory('undo')} aria-label={t(locale, 'workspace.undo')}><Undo2 size={15} /></button>
              <button className="ws-dashboard-action" disabled={!redo.length} onClick={() => void applyHistory('redo')} aria-label={t(locale, 'workspace.redo')}><Redo2 size={15} /></button>
              <div className="relative"><button ref={addButtonRef} className="ws-dashboard-action" onClick={() => setCatalogOpen((open) => !open)} aria-label={t(locale, 'workspace.addWidget')}><Plus size={15} /></button><AddWidgetMenu open={catalogOpen} onOpenChange={setCatalogOpen} triggerRef={addButtonRef} onSelect={(type) => void api.addWidget(type).catch((cause) => setWriteError(String(cause)))} /></div>
              <button className="ws-dashboard-action" onClick={() => setInspectorOpen((open) => !open)} aria-pressed={inspectorOpen}><PanelRight size={15} /></button>
            </>}
            <button type="button" className={`inline-flex h-8 items-center gap-1.5 rounded-lg px-3 text-xs font-medium ${editing ? 'bg-[var(--primary)] text-[var(--primary-foreground)]' : 'bg-[var(--surface)] text-[var(--text-secondary)] shadow-sm hover:text-[var(--text)]'}`} onClick={() => { api.setEditing(!editing); if (editing) setInspectorOpen(false); }}>{editing ? <><Check size={14} />{t(locale, 'workspace.doneEditingBtn')}</> : <><Settings2 size={14} />{t(locale, 'workspace.editLayoutBtn')}</>}</button>
          </div>
        </header>

        {writeError && <div role="alert" className="mx-6 mb-2 flex items-center rounded-lg bg-[var(--danger-soft)] px-3 py-2 text-xs text-[var(--danger)]"><span className="flex-1">{writeError}</span><button onClick={() => setWriteError(null)}><X size={13} /></button></div>}

        <div className="flex min-h-0 flex-1">
          <section className="min-w-0 flex-1 overflow-hidden px-4 pb-4" aria-label="Workspace canvas">
            <Suspense fallback={<DashboardSkeleton compact />}>
              {mode === 'structured' ? <GridWorkspaceView workspaceId={snapshot.workspace.id} viewId="dashboard" layouts={layouts} editable={editing} onLayoutChange={commitGrid} onAddWidget={api.addWidget} onRemoveWidget={api.removeWidget} onActivateItem={setSelectedWidgetId} onWriteError={setWriteError} /> : <FreeCanvasView key={`${snapshot.workspace.id}:${snapshot.revision}`} initialNodes={freeNodes} editable={editing} onCommit={(nodes) => void api.saveFreeLayout({ nodes }).catch((cause) => setWriteError(String(cause)))} onSelectionChange={(ids) => setSelectedWidgetId(ids[0] ?? null)} />}
            </Suspense>
          </section>
          {editing && inspectorOpen && <Inspector mode={mode} theme={snapshot.workspace.theme} selectedWidget={snapshot.widgets.find((widget) => widget.id === selectedWidgetId) ?? null} templates={templates} onMode={(next) => void api.setLayoutMode(next)} onTheme={(theme) => void api.setTheme(theme)} onWidgetConfig={(id, config) => void api.updateWidgetConfig(id, config)} onResetWidget={(id) => void api.resetWidget(id)} onRestore={(id) => void api.restoreTemplate(id)} onSaveTemplate={(name) => void api.saveTemplate(name)} onClose={() => setInspectorOpen(false)} />}
        </div>
      </main>
    </TimeRangeContext.Provider>
  );
}

function TimeRangePicker({ value, onChange }: { value: TimeRange; onChange: (value: TimeRange) => void }) {
  const locale = useLocale();
  const labels: Record<TimeRange, string> = {
    today: t(locale, 'workspace.timeRangeToday'),
    '7d': t(locale, 'workspace.timeRange7d'),
    '30d': t(locale, 'workspace.timeRange30d'),
    '90d': t(locale, 'workspace.timeRange90d'),
  };
  return <div className="flex rounded-lg bg-[var(--surface-hover)] p-0.5" role="group" aria-label={t(locale, 'workspace.dashboardSubtitle')}>{(['today','7d','30d','90d'] as TimeRange[]).map((range) => <button key={range} onClick={() => onChange(range)} className={`rounded-md px-2 py-1 text-[0.6875rem] ${value === range ? 'bg-[var(--surface)] text-[var(--text)] shadow-sm' : 'text-[var(--text-secondary)]'}`}>{labels[range]}</button>)}</div>;
}

function Inspector({ mode, theme, selectedWidget, templates, onMode, onTheme, onWidgetConfig, onResetWidget, onRestore, onSaveTemplate, onClose }: { mode: WorkspaceLayoutMode; theme: 'dark' | 'light'; selectedWidget: NonNullable<ReturnType<typeof useWorkspaceSession>['snapshot']>['widgets'][number] | null; templates: ReturnType<typeof useWorkspaceSession>['templates']; onMode: (mode: WorkspaceLayoutMode) => void; onTheme: (theme: 'dark' | 'light') => void; onWidgetConfig: (id: string, config: Record<string, unknown>) => void; onResetWidget: (id: string) => void; onRestore: (id: string) => void; onSaveTemplate: (name: string) => void; onClose: () => void }) {
  const [name, setName] = useState('');
  const locale = useLocale();
  const widgetLabel = selectedWidget
    ? (() => { const def = getWidget(selectedWidget.widgetType); return def?.titleKey ? t(locale, def.titleKey) : selectedWidget.widgetType; })()
    : '';
  const themeLabels: Record<'dark' | 'light', string> = {
    dark: t(locale, 'workspace.inspectorThemeDark'),
    light: t(locale, 'workspace.inspectorThemeLight'),
  };
  return <aside className="w-80 shrink-0 overflow-auto border-l border-[var(--border-subtle)] bg-[var(--surface)] p-4" aria-label={t(locale, 'workspace.inspector')}><div className="mb-5 flex items-center"><h2 className="text-sm font-semibold">{t(locale, 'workspace.inspector')}</h2><button className="ml-auto ws-dashboard-action" onClick={onClose}><X size={14} /></button></div><section className="space-y-2"><label className="text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-disabled)]">{t(locale, 'workspace.inspectorLayoutLabel')}</label><div className="grid grid-cols-2 gap-1 rounded-lg bg-[var(--surface-hover)] p-1">{(['structured','free'] as WorkspaceLayoutMode[]).map((item) => <button key={item} onClick={() => onMode(item)} className={`rounded-md px-2 py-1.5 text-xs ${mode === item ? 'bg-[var(--surface)] shadow-sm' : ''}`}>{item === 'structured' ? t(locale, 'workspace.inspectorLayoutGrid') : t(locale, 'workspace.inspectorLayoutFree')}</button>)}</div><label className="block pt-2 text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-disabled)]">{t(locale, 'workspace.inspectorThemeLabel')}</label><div className="grid grid-cols-2 gap-1">{(['dark','light'] as const).map((item) => <button key={item} onClick={() => onTheme(item)} className={`rounded-md px-2 py-1.5 text-xs ${theme === item ? 'bg-[var(--surface)] shadow-sm' : ''}`}>{themeLabels[item]}</button>)}</div></section>{selectedWidget && <section className="mt-5 rounded-lg bg-[var(--surface-subtle)] p-3"><p className="text-[0.6875rem] uppercase text-[var(--text-disabled)]">{t(locale, 'workspace.inspectorSelectedWidget')}</p><p className="mt-1 truncate text-xs">{widgetLabel}</p>{selectedWidget.widgetType === 'data_view' && <div className="mt-3 grid grid-cols-2 gap-1">{(['list','table','board','calendar'] as const).map((viewMode) => <button key={viewMode} className="rounded-md bg-[var(--surface)] px-2 py-1 text-[0.6875rem]" onClick={() => onWidgetConfig(selectedWidget.id, { ...selectedWidget.config, settings: { ...((selectedWidget.config.settings as Record<string, unknown>) ?? {}), mode: viewMode } })}>{t(locale, `workspace.dataMode${viewMode.charAt(0).toUpperCase()}${viewMode.slice(1)}` as `workspace.dataMode${'List' | 'Table' | 'Board' | 'Calendar'}`)}</button>)}</div>}<button className="mt-3 flex w-full items-center justify-center gap-1 rounded-md bg-[var(--surface)] px-2 py-1.5 text-[0.6875rem]" onClick={() => onResetWidget(selectedWidget.id)}><RotateCcw size={12} />{t(locale, 'workspace.inspectorResetWidget')}</button></section>}<section className="mt-6 space-y-2"><label className="text-[0.6875rem] font-medium uppercase tracking-wide text-[var(--text-disabled)]">{t(locale, 'workspace.inspectorTemplatesLabel')}</label>{templates.map((template) => <button key={template.id} onClick={() => onRestore(template.id)} className="flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left text-xs hover:bg-[var(--surface-hover)]"><LayoutDashboard size={14} /><span className="min-w-0 flex-1 truncate">{template.name}</span><RotateCcw size={12} /></button>)}<div className="flex gap-1 pt-2"><input value={name} onChange={(event) => setName(event.target.value)} placeholder={t(locale, 'workspace.inspectorTemplateNamePlaceholder')} className="min-w-0 flex-1 rounded-lg border border-[var(--border)] bg-transparent px-2 text-xs" /><button className="ws-dashboard-action" disabled={!name.trim()} onClick={() => { onSaveTemplate(name.trim()); setName(''); }}><Save size={14} /></button></div></section></aside>;
}

function DashboardSkeleton({ compact = false }: { compact?: boolean }) {
  return <div className={`grid h-full gap-4 p-6 ${compact ? 'grid-cols-2' : 'grid-cols-3'}`}>{Array.from({ length: compact ? 4 : 6 }, (_, index) => <Skeleton key={index} variant="card" lines={3} />)}</div>;
}
