'use client';

/**
 * WorkspaceInspector (C-023..C-026) — context-aware inspector host.
 *
 * Reuses the shared ResizableRightPanel (the existing rail is the base, not a
 * copied Twenty inspector). It renders a tabbed rail whose content depends on
 * the active view kind:
 *  - grid  → view + widget layout facts
 *  - canvas → selection / z-order / grouping actions
 *  - data  → fields + view mode facts
 */

import {
  ArrowDownToLine,
  ArrowUpToLine,
  Columns3,
  Layers,
  LayoutGrid,
  Trash2,
  Ungroup,
} from 'lucide-react';
import ResizableRightPanel from '@/components/ui/ResizableRightPanel';
import type { WorkspaceViewConfig } from '@/lib/workspace/views/types';

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
}: WorkspaceInspectorProps) {
  if (!open || !activeView) return null;

  const tabs = [
    { id: 'properties', label: 'Properties', icon: <Columns3 size={13} /> },
    { id: 'appearance', label: 'Appearance', icon: <LayoutGrid size={13} /> },
  ];

  return (
    <ResizableRightPanel
      open={open}
      width={width}
      onResize={onResize}
      onClose={onClose}
      tabs={tabs}
      activeTabId="properties"
      ariaLabel="Workspace inspector"
      resizeLabel="Resize inspector"
    >
      <div className="space-y-4 p-3">
        <HeaderLine viewTitle={activeView.title} kind={activeView.kind} />

        {activeView.kind === 'grid' && (
          <GridSection gridEditing={gridEditing} onEditGrid={onEditGrid} layoutCount={layoutItemCount(activeView)} />
        )}

        {activeView.kind === 'canvas' && (
          <CanvasSection selectionIds={canvasSelectionIds} onAction={onCanvasAction} />
        )}

        {activeView.kind === 'data' && (
          <DataSection
            mode={activeView.data?.mode}
            columns={activeView.data?.columns}
            groupBy={activeView.data?.groupBy}
          />
        )}

        <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] p-2.5 text-[0.625rem] leading-relaxed text-[var(--text-disabled)]">
          Inspector state is derived from the active view snapshot. Changes
          commit to the session snapshot (debounced), never per pointer.
        </div>
      </div>
    </ResizableRightPanel>
  );
}

function HeaderLine({ viewTitle, kind }: { viewTitle: string; kind: string }) {
  return (
    <div className="flex items-center gap-2">
      <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-[var(--primary-soft)] text-[var(--primary)]">
        <LayoutGrid size={15} />
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm font-medium text-[var(--text)]">{viewTitle}</div>
        <div className="text-[0.625rem] uppercase tracking-wide text-[var(--text-disabled)]">{kind} view</div>
      </div>
    </div>
  );
}

function SectionCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)]">
      <div className="border-b border-[var(--border-subtle)] px-3 py-2 text-[0.625rem] font-semibold uppercase tracking-wide text-[var(--text-disabled)]">
        {title}
      </div>
      <div className="p-3">{children}</div>
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
  return (
    <SectionCard title="Grid view">
      <div className="space-y-3">
        <Field label="Layout items" value={String(layoutCount)} />
        <Field label="Columns (lg/md/sm)" value="12 / 8 / 4" />
        <Field label="Persistence" value="On drag/resize stop" />
        {onEditGrid && (
          <button
            type="button"
            onClick={() => onEditGrid(!gridEditing)}
            className={`w-full rounded-lg px-3 py-2 text-xs font-medium transition-colors ${
              gridEditing
                ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                : 'bg-[var(--surface-hover)] text-[var(--text)]'
            }`}
          >
            {gridEditing ? 'Done editing' : 'Edit layout'}
          </button>
        )}
      </div>
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
  return (
    <SectionCard title="Canvas selection">
      <div className="space-y-3">
        <Field label="Selected" value={selectionIds.length === 0 ? 'None' : `${selectionIds.length} node(s)`} />
        <div className="flex flex-wrap gap-1.5">
          <ActionButton icon={<Layers size={12} />} label="Group" onClick={() => onAction?.({ type: 'group' })} />
          <ActionButton icon={<Ungroup size={12} />} label="Ungroup" onClick={() => onAction?.({ type: 'ungroup' })} />
          <ActionButton icon={<ArrowUpToLine size={12} />} label="Front" onClick={() => onAction?.({ type: 'front' })} />
          <ActionButton icon={<ArrowDownToLine size={12} />} label="Back" onClick={() => onAction?.({ type: 'back' })} />
        </div>
        <button
          type="button"
          onClick={() => onAction?.({ type: 'delete' })}
          disabled={selectionIds.length === 0}
          className="inline-flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/10 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Trash2 size={12} />
          Delete selection
        </button>
      </div>
    </SectionCard>
  );
}

function DataSection({
  mode,
  columns,
  groupBy,
}: {
  mode?: string;
  columns?: string[];
  groupBy?: string | null;
}) {
  return (
    <SectionCard title="Data view">
      <div className="space-y-3">
        <Field label="Mode" value={mode ?? 'list'} />
        <Field label="Fields" value={columns?.length ? String(columns.length) : '0'} />
        <Field label="Group by" value={groupBy ?? 'None'} />
      </div>
    </SectionCard>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-xs text-[var(--text-secondary)]">{label}</span>
      <span className="truncate text-xs font-medium tabular-nums text-[var(--text)]">{value}</span>
    </div>
  );
}

function ActionButton({ icon, label, onClick }: { icon: React.ReactNode; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex items-center gap-1 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-hover)] px-2 py-1.5 text-[0.625rem] text-[var(--text-secondary)] transition-colors hover:text-[var(--text)]"
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
