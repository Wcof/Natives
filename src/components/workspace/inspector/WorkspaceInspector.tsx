'use client';

/**
 * WorkspaceInspector (C-023..C-026) — context-aware inspector host.
 *
 * Twenty-inspired modular record inspector layout:
 *  - Organized field sections with property icons and tag chips.
 *  - Interactive tab navigation (Properties, Layout, Appearance).
 *  - Smooth drawer controls and action buttons.
 *  - Dense, legible typography with semantic tokens.
 */

import { useState } from 'react';
import { useLocale, t, type Locale } from '@/i18n';
import {
  ArrowDownToLine,
  ArrowUpToLine,
  Columns3,
  Hash,
  Layers,
  LayoutGrid,
  Palette,
  Sliders,
  Sparkles,
  Trash2,
  Ungroup,
} from 'lucide-react';
import ResizableRightPanel from '@/components/ui/ResizableRightPanel';
import type { DataViewState } from '@/lib/workspace/views/types';
import type { WorkspaceViewConfig } from '@/lib/workspace/views/types';
import { kindLabel } from '../tabs/tabStripModel';

export interface CanvasInspectorAction {
  type: 'delete' | 'group' | 'ungroup' | 'front' | 'back';
}

export interface WorkspaceInspectorProps {
  open: boolean;
  width: number;
  onResize: (width: number) => void;
  onClose: () => void;
  activeView: WorkspaceViewConfig | null;
  canvasSelectionIds?: string[];
  onCanvasAction?: (action: CanvasInspectorAction) => void;
  onEditGrid?: (editing: boolean) => void;
  gridEditing?: boolean;
  /** 真实双向绑定：Inspector 修改 data view 状态时回写（毫秒级重绘 + 持久化）。 */
  onDataStateChange?: (patch: Partial<DataViewState>) => void;
  /** 真实双向绑定：重命名当前视图标题。 */
  onRenameView?: (title: string) => void;
}

export default function WorkspaceInspector({
  open,
  width,
  onResize,
  onClose,
  activeView,
  canvasSelectionIds = [],
  onCanvasAction,
  onEditGrid,
  gridEditing = false,
  onDataStateChange,
  onRenameView,
}: WorkspaceInspectorProps) {
  const locale = useLocale();
  const [activeTab, setActiveTab] = useState<'properties' | 'appearance'>('properties');

  if (!open || !activeView) return null;

  const tabs = [
    { id: 'properties', label: t(locale, 'workspace.properties'), icon: <Columns3 size={13} /> },
    { id: 'appearance', label: t(locale, 'workspace.appearance'), icon: <Palette size={13} /> },
  ];

  return (
    <ResizableRightPanel
      open={open}
      width={width}
      onResize={onResize}
      onClose={onClose}
      tabs={tabs}
      activeTabId={activeTab}
      onTabChange={(id) => setActiveTab(id as 'properties' | 'appearance')}
      ariaLabel={t(locale, 'workspace.inspector')}
      resizeLabel={t(locale, 'workspace.toggleInspector')}
    >
      <div className="space-y-4 p-3.5">
        <HeaderLine
          viewTitle={activeView.title}
          kind={activeView.kind}
          onRename={onRenameView}
        />

        {activeTab === 'properties' && (
          <>
            {activeView.kind === 'grid' && (
              <GridSection gridEditing={gridEditing} onEditGrid={onEditGrid} layoutCount={layoutItemCount(activeView)} />
            )}

            {activeView.kind === 'canvas' && (
              <CanvasSection selectionIds={canvasSelectionIds} onAction={onCanvasAction} />
            )}

            {activeView.kind === 'data' && (
              <DataSection
                mode={activeView.data?.mode}
                columns={activeView.data?.columns ?? []}
                hiddenColumns={activeView.data?.hiddenColumns ?? []}
                groupBy={activeView.data?.groupBy ?? null}
                onChange={onDataStateChange}
              />
            )}
          </>
        )}

        {activeTab === 'appearance' && (
          <AppearanceSection kind={activeView.kind} />
        )}

        <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-hover)]/40 p-3 text-[0.6875rem] leading-relaxed text-[var(--text-secondary)]">
          <div className="flex items-center gap-1.5 font-medium text-[var(--text)] mb-1">
            <Sparkles size={12} className="text-[var(--primary)]" />
            <span>{t(locale, 'workspace.persistenceHint')}</span>
          </div>
          {t(locale, 'workspace.persistenceDesc')}
        </div>
      </div>
    </ResizableRightPanel>
  );
}

function HeaderLine({ viewTitle, kind, onRename }: { viewTitle: string; kind: string; onRename?: (title: string) => void }) {
  const locale = useLocale();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(viewTitle);

  const commit = () => {
    setEditing(false);
    const next = draft.trim();
    if (next && next !== viewTitle) onRename?.(next);
  };

  return (
    <div className="flex items-center gap-2.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] p-3 shadow-sm">
      <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-[var(--primary-soft)] text-[var(--primary)] shrink-0">
        <LayoutGrid size={16} />
      </div>
      <div className="min-w-0 flex-1">
        {editing ? (
          <input
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commit();
              if (e.key === 'Escape') {
                setDraft(viewTitle);
                setEditing(false);
              }
            }}
            className="w-full rounded border border-[var(--border)] bg-[var(--surface-hover)] px-1.5 py-0.5 text-sm font-semibold text-[var(--text)] outline-none focus:border-[var(--primary)]"
            aria-label={t(locale, 'workspace.renameView')}
          />
        ) : (
          <button
            type="button"
            onClick={() => {
              setDraft(viewTitle);
              setEditing(true);
            }}
            className="block max-w-full truncate rounded px-1 py-0.5 text-left text-sm font-semibold text-[var(--text)] transition-colors hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]"
            title={t(locale, 'workspace.renameView')}
          >
            {localizeTitle(viewTitle, locale)}
          </button>
        )}
        <div className="flex items-center gap-1.5 mt-0.5">
          <span className="inline-block h-1.5 w-1.5 rounded-full bg-[var(--success)]" />
          <span className="text-[0.6875rem] uppercase tracking-wider text-[var(--text-secondary)] font-medium">
            {kindLabel(kind as 'grid' | 'canvas' | 'data', locale)}
          </span>
        </div>
      </div>
    </div>
  );
}

function SectionCard({ title, icon, children }: { title: string; icon?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] overflow-hidden shadow-sm">
      <div className="flex items-center gap-1.5 border-b border-[var(--border-subtle)] px-3 py-2 text-[0.6875rem] font-semibold uppercase tracking-wider text-[var(--text-secondary)] bg-[var(--surface-hover)]/30">
        {icon}
        <span>{title}</span>
      </div>
      <div className="p-3 divide-y divide-[var(--border-subtle)]">{children}</div>
    </section>
  );
}

function GridSection({
  gridEditing,
  onEditGrid,
  layoutCount,
}: {

  gridEditing: boolean;
  onEditGrid?: (editing: boolean) => void;
  layoutCount: number;
}) {
  const locale = useLocale();
  return (
    <SectionCard title={t(locale, 'workspace.layoutSpec')} icon={<Sliders size={12} className="text-[var(--primary)]" />}>
      <div className="space-y-2.5 pb-2.5">
        <TwentyField label={t(locale, 'workspace.placedWidgets')} value={String(layoutCount)} icon={<Hash size={12} />} />
        <TwentyField label={t(locale, 'workspace.gridColumns')} value="12 / 8 / 4 (lg/md/sm)" />
        <TwentyField label={t(locale, 'workspace.compaction')} value={t(locale, 'workspace.collisionFree')} />
      </div>
      {onEditGrid && (
        <div className="pt-2.5">
          <button
            type="button"
            onClick={() => onEditGrid(!gridEditing)}
            className={`w-full rounded-lg px-3 py-2 text-xs font-semibold transition-all ${
              gridEditing
                ? 'bg-[var(--primary)] text-[var(--primary-foreground)] shadow-sm'
                : 'bg-[var(--surface-hover)] text-[var(--text)] hover:bg-[var(--primary-soft)] hover:text-[var(--primary)]'
            }`}
          >
            {gridEditing ? t(locale, 'workspace.doneEditing') : t(locale, 'workspace.editLayout')}
          </button>
        </div>
      )}
    </SectionCard>
  );
}

function CanvasSection({
  selectionIds,
  onAction,
}: {

  selectionIds: string[];
  onAction?: (action: CanvasInspectorAction) => void;
}) {
  const locale = useLocale();
  return (
    <SectionCard title={t(locale, 'workspace.selectedNodes')} icon={<Layers size={12} className="text-[var(--primary)]" />}>
      <div className="space-y-2.5 pb-2.5">
        <TwentyField
          label={t(locale, 'workspace.inspectorSelection')}
          value={selectionIds.length === 0 ? t(locale, 'workspace.none') : t(locale, 'workspace.selectionCount', { count: selectionIds.length })}
        />
      </div>
      <div className="pt-2.5 space-y-2">
        <div className="text-[0.625rem] font-semibold uppercase tracking-wider text-[var(--text-disabled)]">
          {t(locale, 'workspace.nodeActions')}
        </div>
        <div className="grid grid-cols-2 gap-1.5">
          <ActionButton icon={<Layers size={12} />} label={t(locale, 'workspace.group')} onClick={() => onAction?.({ type: 'group' })} disabled={selectionIds.length < 2} />
          <ActionButton icon={<Ungroup size={12} />} label={t(locale, 'workspace.ungroup')} onClick={() => onAction?.({ type: 'ungroup' })} disabled={selectionIds.length === 0} />
          <ActionButton icon={<ArrowUpToLine size={12} />} label={t(locale, 'workspace.toFront')} onClick={() => onAction?.({ type: 'front' })} disabled={selectionIds.length === 0} />
          <ActionButton icon={<ArrowDownToLine size={12} />} label={t(locale, 'workspace.toBack')} onClick={() => onAction?.({ type: 'back' })} disabled={selectionIds.length === 0} />
        </div>
        <button
          type="button"
          onClick={() => onAction?.({ type: 'delete' })}
          disabled={selectionIds.length === 0}
          className="w-full inline-flex items-center justify-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/10 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Trash2 size={12} />
          {t(locale, 'workspace.deleteSelection')}
        </button>
      </div>
    </SectionCard>
  );
}

function DataSection({
  mode,
  columns,
  hiddenColumns,
  groupBy,
  onChange,
}: {
  mode?: string;
  columns: string[];
  hiddenColumns: string[];
  groupBy?: string | null;
  onChange?: (patch: Partial<DataViewState>) => void;
}) {
  const locale = useLocale();

  const toggleColumn = (col: string) => {
    if (!onChange) return;
    const hidden = hiddenColumns.includes(col);
    onChange({
      hiddenColumns: hidden
        ? hiddenColumns.filter((c) => c !== col)
        : [...hiddenColumns, col],
    });
  };

  const modeLabel = (m: string) =>
    t(locale, `workspace.dataMode${m.charAt(0).toUpperCase()}${m.slice(1)}`);

  return (
    <SectionCard title={t(locale, 'workspace.recordSchema')} icon={<Columns3 size={12} className="text-[var(--primary)]" />}>
      <div className="space-y-2.5 pb-2.5">
        {/* 视图模式切换（真实写入 state） */}
        <div>
          <div className="mb-1.5 text-[0.6875rem] font-medium text-[var(--text-secondary)]">
            {t(locale, 'workspace.viewType')}
          </div>
          <div className="flex items-center gap-1">
            {(['list', 'table', 'board', 'calendar'] as const).map((m) => (
              <button
                key={m}
                type="button"
                disabled={!onChange}
                onClick={() => onChange?.({ mode: m })}
                className={`rounded-md px-2 py-1 text-[0.6875rem] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                  mode === m
                    ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'bg-[var(--surface-hover)] text-[var(--text-secondary)] hover:text-[var(--text)]'
                }`}
              >
                {modeLabel(m)}
              </button>
            ))}
          </div>
        </div>

        {/* 列显隐（真实双向绑定：勾选即回写并重绘） */}
        {columns.length > 0 && (
          <div>
            <div className="mb-1.5 text-[0.6875rem] font-medium text-[var(--text-secondary)]">
              {t(locale, 'workspace.visibleColumns')}
            </div>
            <div className="space-y-1">
              {columns.map((col) => {
                const hidden = hiddenColumns.includes(col);
                return (
                  <label
                    key={col}
                    className={`flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-1 text-xs transition-colors ${
                      hidden ? 'text-[var(--text-disabled)]' : 'text-[var(--text)]'
                    } hover:bg-[var(--surface-hover)] ${onChange ? '' : 'pointer-events-none opacity-60'}`}
                  >
                    <span className="flex items-center gap-1.5">
                      <input
                        type="checkbox"
                        checked={!hidden}
                        disabled={!onChange}
                        onChange={() => toggleColumn(col)}
                        className="h-3.5 w-3.5 accent-[var(--primary)]"
                      />
                      {localizeColumn(col, locale)}
                    </span>
                    <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-[0.625rem] tabular-nums">
                      {col}
                    </span>
                  </label>
                );
              })}
            </div>
          </div>
        )}

        {/* 分组字段下拉（真实写入 state） */}
        <div>
          <div className="mb-1.5 text-[0.6875rem] font-medium text-[var(--text-secondary)]">
            {t(locale, 'workspace.groupAttribute')}
          </div>
          <select
            disabled={!onChange}
            value={groupBy ?? ''}
            onChange={(e) => onChange?.({ groupBy: e.target.value || null })}
            className="w-full rounded-lg border border-[var(--border)] bg-[var(--surface)] px-2 py-1.5 text-xs text-[var(--text)] outline-none transition-colors focus:border-[var(--primary)] disabled:opacity-40"
          >
            <option value="">—</option>
            {columns.map((col) => (
              <option key={col} value={col}>
                {localizeColumn(col, locale)}
              </option>
            ))}
          </select>
        </div>
      </div>
    </SectionCard>
  );
}

function AppearanceSection({ kind }: { kind: string }) {
  const locale = useLocale();
  return (
    <SectionCard title={t(locale, 'workspace.surfacePolicy')} icon={<Palette size={12} className="text-[var(--primary)]" />}>
      <div className="space-y-2.5 pb-2.5">
        <TwentyField label={t(locale, 'workspace.surfacePolicy')} value={t(locale, 'workspace.materialPolicy')} />
        <TwentyField label={t(locale, 'workspace.backdropBlur')} value={t(locale, 'workspace.backdropBlur')} />
        <TwentyField label={t(locale, 'workspace.shadowElevation')} value={t(locale, 'workspace.shadowElevation')} />
        <TwentyField label={t(locale, 'workspace.targetSurface')} value={t(locale, 'workspace.targetSurfaceValue', { kind: kindLabel(kind as 'grid' | 'canvas' | 'data', locale) })} />
      </div>
    </SectionCard>
  );
}

function TwentyField({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon?: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2 py-0.5">
      <div className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
        {icon}
        <span>{label}</span>
      </div>
      <span className="rounded-md bg-[var(--surface-hover)] px-2 py-0.5 text-xs font-medium tabular-nums text-[var(--text)]">
        {value}
      </span>
    </div>
  );
}

function ActionButton({
  icon,
  label,
  onClick,
  disabled = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="inline-flex items-center justify-center gap-1 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)] px-2 py-1.5 text-xs font-medium text-[var(--text-secondary)] transition-colors hover:text-[var(--text)] hover:border-[var(--primary)] disabled:opacity-40 disabled:cursor-not-allowed"
    >
      {icon}
      {label}
    </button>
  );
}

function layoutItemCount(view: WorkspaceViewConfig): number {
  const lg = view.gridLayouts?.lg;
  return lg?.length ?? 0;
}

function localizeColumn(col: string, locale: Locale): string {
  switch (col) {
    case 'name':
      return t(locale, 'workspace.colName');
    case 'status':
      return t(locale, 'workspace.colStatus');
    case 'assignee':
      return t(locale, 'workspace.colAssignee');
    case 'due':
      return t(locale, 'workspace.colDue');
    default:
      return col;
  }
}

function localizeTitle(title: string, locale: Locale): string {
  if (title === 'Start Here' || title.toLowerCase().includes('start')) {
    return t(locale, 'workspace.viewStartHere');
  }
  if (title === 'Idea Board' || title.toLowerCase().includes('idea')) {
    return t(locale, 'workspace.viewIdeaBoard');
  }
  if (title === 'Tasks' || title.toLowerCase().includes('task')) {
    return t(locale, 'workspace.viewTasks');
  }
  return title;
}
