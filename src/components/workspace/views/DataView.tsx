'use client';

/**
 * DataView (C-027..C-031) — List / Table / Board / Calendar controlled views.
 *
 * - The view is fully controlled: mode, columns, sort, filter and group-by
 *   live in DataViewState and are persisted per view id (debounced).
 * - Plane-inspired Kanban board dragging interaction (cross-column drag & drop,
 *   hover placeholder, grip handle reveal, clean animation).
 * - Dense/readable by default; only semantic tokens are used.
 */

import { useMemo, useState } from 'react';
import { useLocale, t, type Locale } from '@/i18n';
import {
  ArrowDown,
  ArrowUp,
  CalendarDays,
  ChevronDown,
  Columns3,
  GripVertical,
  List,
  Pencil,
  Plus,
  Rows3,
  Table2,
  Trash2,
  X,
} from 'lucide-react';
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCorners,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import type { DataFilter, DataViewState } from '@/lib/workspace/views/types';

export type DataRow = Record<string, string | number | boolean | null>;

export interface DataViewProps {
  viewId: string;
  state: DataViewState;
  onStateChange: (patch: Partial<DataViewState>) => void;
  rows?: DataRow[];
}

export default function DataView({ viewId, state, onStateChange, rows = [] }: DataViewProps) {
  const locale = useLocale();
  const modeLabel = (mode: DataViewState['mode']) =>
    t(locale, `workspace.dataMode${mode.charAt(0).toUpperCase()}${mode.slice(1)}`);
  const [localRows, setLocalRows] = useState<DataRow[]>(rows);
  const [menuOpen, setMenuOpen] = useState<'columns' | 'sort' | 'filter' | null>(null);

  const filtered = useMemo(() => {
    let out = localRows;
    for (const f of state.filters) {
      out = out.filter((row) => applyFilter(row, f));
    }
    if (state.sort?.field) {
      const { field, dir } = state.sort;
      out = [...out].sort((a, b) => {
        const av = a[field];
        const bv = b[field];
        if (av == null && bv == null) return 0;
        if (av == null) return 1;
        if (bv == null) return -1;
        const cmp = typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av).localeCompare(String(bv));
        return dir === 'asc' ? cmp : -cmp;
      });
    }
    return out;
  }, [localRows, state.filters, state.sort]);

  const groups = useMemo(() => {
    const by = state.groupBy || 'status';
    const map = new Map<string, DataRow[]>();
    for (const row of filtered) {
      const key = row[by] == null || row[by] === '' ? 'Unassigned' : String(row[by]);
      const list = map.get(key) ?? [];
      list.push(row);
      map.set(key, list);
    }
    return [...map.entries()].map(([key, items]) => ({ key, items }));
  }, [filtered, state.groupBy]);

  const visibleColumns = state.columns.filter((col) => !state.hiddenColumns.includes(col));

  const toggleMenu = (name: 'columns' | 'sort' | 'filter') =>
    setMenuOpen((cur) => (cur === name ? null : name));

  const handleRowMove = (rowId: string, targetGroupKey: string) => {
    const groupField = state.groupBy || 'status';
    setLocalRows((prev) =>
      prev.map((r) => (String(r.id) === rowId ? { ...r, [groupField]: targetGroupKey } : r)),
    );
  };

  const handleUpdateRow = (rowId: string, patch: Partial<DataRow>) => {
    setLocalRows((prev) =>
      prev.map((r) => {
        if (String(r.id) !== rowId) return r;
        const merged: DataRow = { ...r };
        for (const [key, value] of Object.entries(patch)) {
          if (value !== undefined) merged[key] = value;
        }
        return merged;
      }),
    );
  };

  const handleDeleteRow = (rowId: string) => {
    setLocalRows((prev) => prev.filter((r) => String(r.id) !== rowId));
  };

  const handleAddCard = (groupKey: string) => {
    const groupField = state.groupBy || 'status';
    const newId = `t-${Date.now().toString(36)}`;
    const newRow: DataRow = {
      id: newId,
      name: t(locale, 'workspace.newTask'),
      [groupField]: groupKey,
      assignee: t(locale, 'workspace.assigneeYou'),
      due: new Date().toISOString().split('T')[0] ?? '2026-01-01',
    };
    setLocalRows((prev) => [...prev, newRow]);
  };

  /** @dnd-kit 列内重排：仅调整同组内相对顺序，其余行保持原序。 */
  const handleReorderRows = (groupId: string, fromId: string, toId: string) => {
    setLocalRows((prev) => {
      const groupField = state.groupBy || 'status';
      const indices = prev
        .map((r, i) => ({ r, i }))
        .filter(({ r }) => String(r[groupField] ?? '') === groupId)
        .map(({ i }) => i);
      const fromIdx = prev.findIndex((r) => String(r.id) === fromId);
      const toIdx = prev.findIndex((r) => String(r.id) === toId);
      if (fromIdx < 0 || toIdx < 0) return prev;
      const next = [...prev];
      const [moved] = next.splice(fromIdx, 1);
      if (!moved) return prev;
      next.splice(toIdx, 0, moved);
      // Keep non-group rows in place: reorder is only meaningful within the group.
      if (indices.includes(fromIdx) && indices.includes(toIdx)) return next;
      return prev;
    });
  };

  const modeButton = (mode: DataViewState['mode'], label: string, icon: React.ReactNode) => (
    <button
      key={mode}
      type="button"
      onClick={() => onStateChange({ mode })}
      className={`inline-flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs transition-colors ${
        state.mode === mode
          ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
          : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
      }`}
      title={t(locale, 'workspace.switchToView', { label })}
    >
      {icon}
      <span className="hidden sm:inline">{label}</span>
    </button>
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden" data-testid={`data-view-${viewId}`}>
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-[var(--border-subtle)] px-2">
        {modeButton('list', modeLabel('list'), <List size={14} />)}
        {modeButton('table', modeLabel('table'), <Table2 size={14} />)}
        {modeButton('board', modeLabel('board'), <Columns3 size={14} />)}
        {modeButton('calendar', modeLabel('calendar'), <CalendarDays size={14} />)}

        <div className="ml-auto flex items-center gap-1">
          <MenuButton
            label={`${t(locale, 'workspace.sort')}${state.sort ? `: ${state.sort.field}` : ''}`}
            icon={state.sort?.dir === 'desc' ? <ArrowDown size={13} /> : <ArrowUp size={13} />}
            open={menuOpen === 'sort'}
            onClick={() => toggleMenu('sort')}
          >
            {state.columns.map((col) => (
              <MenuRow
                key={col}
                active={state.sort?.field === col}
                label={localizeColumn(col, locale)}
                onClick={() =>
                  onStateChange({
                    sort:
                      state.sort?.field === col && state.sort.dir === 'asc'
                        ? { field: col, dir: 'desc' }
                        : { field: col, dir: 'asc' },
                  })
                }
              />
            ))}
          </MenuButton>

          <MenuButton
            label={`${t(locale, 'workspace.groupBy')}${state.groupBy ? `: ${state.groupBy}` : ''}`}
            icon={<Columns3 size={13} />}
            open={menuOpen === 'filter'}
            onClick={() => toggleMenu('filter')}
          >
            <MenuRow active={!state.groupBy} label={t(locale, 'workspace.none')} onClick={() => onStateChange({ groupBy: undefined })} />
            {state.columns.map((col) => (
              <MenuRow
                key={col}
                active={state.groupBy === col}
                label={localizeColumn(col, locale)}
                onClick={() => onStateChange({ groupBy: col })}
              />
            ))}
          </MenuButton>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {state.mode === 'list' && <ListView rows={filtered} columns={visibleColumns} />}
        {state.mode === 'table' && (
          <TableView rows={filtered} columns={visibleColumns} sort={state.sort || undefined} onSort={(s) => onStateChange({ sort: s })} />
        )}
        {state.mode === 'board' && (
          <BoardView
            rows={localRows}
            groups={groups}
            groupBy={state.groupBy || 'status'}
            onRowMove={handleRowMove}
            onAddCard={handleAddCard}
            onUpdateRow={handleUpdateRow}
            onDeleteRow={handleDeleteRow}
            onReorderRows={handleReorderRows}
          />
        )}
        {state.mode === 'calendar' && <CalendarView rows={filtered} field="due" />}
      </div>
    </div>
  );
}

function ListView({ rows, columns }: { rows: DataRow[]; columns: string[] }) {
  if (rows.length === 0) return <EmptyData />;
  return (
    <ul className="divide-y divide-[var(--border-subtle)] p-2">
      {rows.map((row) => (
        <li
          key={String(row.id)}
          className="flex items-center justify-between gap-3 rounded-lg px-3 py-2 text-xs transition-colors hover:bg-[var(--surface-hover)]"
        >
          <span className="font-medium text-[var(--text)]">{primaryText(row, columns)}</span>
          <div className="flex items-center gap-2 text-[0.6875rem] text-[var(--text-secondary)]">
            {columns.slice(1).map((col) => (
              <span key={col} className="rounded bg-[var(--surface)] px-1.5 py-0.5">
                {String(row[col] ?? '—')}
              </span>
            ))}
          </div>
        </li>
      ))}
    </ul>
  );
}

function TableView({
  rows,
  columns,
  sort,
  onSort,
}: {
  rows: DataRow[];
  columns: string[];
  sort?: { field: string; dir: 'asc' | 'desc' };
  onSort: (sort: { field: string; dir: 'asc' | 'desc' }) => void;
}) {
  const locale = useLocale();
  return (
    <table className="w-full border-collapse text-left text-xs">
      <thead>
        <tr className="border-b border-[var(--border-subtle)] bg-[var(--surface-hover)]/40 text-[0.6875rem] text-[var(--text-disabled)]">
          {columns.map((col) => (
            <th key={col} className="px-3 py-2 font-medium">
              <button
                type="button"
                onClick={() =>
                  onSort({
                    field: col,
                    dir: sort?.field === col && sort.dir === 'asc' ? 'desc' : 'asc',
                  })
                }
                className="inline-flex items-center gap-1 hover:text-[var(--text)]"
              >
                <span>{localizeColumn(col, locale)}</span>
                {sort?.field === col &&
                  (sort.dir === 'asc' ? <ArrowUp size={11} /> : <ArrowDown size={11} />)}
              </button>
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={String(row.id)} className="border-b border-[var(--border-subtle)] hover:bg-[var(--surface-hover)]/50">
            {columns.map((col, i) => (
              <td key={col} className={`px-3 py-2 ${i === 0 ? 'font-medium text-[var(--text)]' : 'text-[var(--text-secondary)]'}`}>
                {String(row[col] ?? '—')}
              </td>
            ))}
          </tr>
        ))}
        {rows.length === 0 && (
          <tr>
            <td colSpan={columns.length} className="px-3 py-8">
              <EmptyData />
            </td>
          </tr>
        )}
      </tbody>
    </table>
  );
}

function BoardView({
  rows,
  groups,
  groupBy,
  onRowMove,
  onAddCard,
  onUpdateRow,
  onDeleteRow,
  onReorderRows,
}: {
  rows: DataRow[];
  groups: { key: string; items: DataRow[] }[];
  groupBy: string;
  onRowMove: (rowId: string, newGroupKey: string) => void;
  onAddCard: (groupKey: string) => void;
  onUpdateRow: (rowId: string, patch: Partial<DataRow>) => void;
  onDeleteRow: (rowId: string) => void;
  onReorderRows: (groupId: string, fromId: string, toId: string) => void;
}) {
  const [activeId, setActiveId] = useState<string | null>(null);
  const [modal, setModal] = useState<{ row: DataRow; groupKey: string } | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const activeRow = activeId ? rows.find((r) => String(r.id) === activeId) ?? null : null;

  const handleDragStart = (event: DragStartEvent) => {
    setActiveId(String(event.active.id));
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    setActiveId(null);
    if (!over) return;
    const activeRowId = String(active.id);
    const overId = String(over.id);
    if (activeRowId === overId) return;

    const columnPrefix = 'column-';
    if (overId.startsWith(columnPrefix)) {
      // 拖到列容器上：纯跨列移动（插到列尾）。
      const groupKey = overId.slice(columnPrefix.length);
      const current = rows.find((r) => String(r.id) === activeRowId)?.[groupBy];
      if (String(current ?? '') !== groupKey) onRowMove(activeRowId, groupKey);
      return;
    }

    // over 是卡片：同列 → 列内重排；异列 → 跨列 + 插入位置。
    const activeRowData = rows.find((r) => String(r.id) === activeRowId);
    const overRow = rows.find((r) => String(r.id) === overId);
    if (!activeRowData || !overRow) return;
    const activeGroup = String(activeRowData[groupBy] ?? '');
    const overGroup = String(overRow[groupBy] ?? '');
    if (activeGroup !== overGroup) {
      onRowMove(activeRowId, overGroup);
      onReorderRows(overGroup, activeRowId, overId);
    } else {
      onReorderRows(activeGroup, activeRowId, overId);
    }
  };

  return (
    <div className="flex h-full items-start gap-3 overflow-x-auto p-3">
      <DndContext
        sensors={sensors}
        collisionDetection={closestCorners}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDragCancel={() => setActiveId(null)}
      >
        {groups.map((group) => (
          <BoardColumn
            key={group.key}
            groupKey={group.key}
            rows={group.items}
            groupBy={groupBy}
            onAddCard={onAddCard}
            onEdit={setModal}
            onDelete={onDeleteRow}
          />
        ))}
        <DragOverlay dropAnimation={null}>
          {activeRow ? (
            <div className="w-64 rounded-lg border border-[var(--border-strong)] bg-[var(--surface)] p-2.5 shadow-lg ring-1 ring-[var(--primary-soft)] opacity-90">
              <p className="text-xs font-medium leading-snug text-[var(--text)]">
                {primaryText(activeRow, ['name'])}
              </p>
            </div>
          ) : null}
        </DragOverlay>
      </DndContext>
      {groups.length === 0 && <EmptyData />}
      {modal && (
        <TaskEditorModal
          row={modal.row}
          groupKey={modal.groupKey}
          groupBy={groupBy}
          groups={groups.map((g) => g.key)}
          onSave={(patch) => {
            onUpdateRow(String(modal.row.id), patch);
            setModal(null);
          }}
          onDelete={() => {
            onDeleteRow(String(modal.row.id));
            setModal(null);
          }}
          onClose={() => setModal(null)}
        />
      )}
    </div>
  );
}

/** 看板列：droppable 容器 + SortableContext（列内排序）。 */
function BoardColumn({
  groupKey,
  rows,
  groupBy,
  onAddCard,
  onEdit,
  onDelete,
}: {
  groupKey: string;
  rows: DataRow[];
  groupBy: string;
  onAddCard: (groupKey: string) => void;
  onEdit: (entry: { row: DataRow; groupKey: string }) => void;
  onDelete: (rowId: string) => void;
}) {
  const locale = useLocale();
  const { setNodeRef, isOver } = useDroppable({ id: `column-${groupKey}` });
  return (
    <div
      ref={setNodeRef}
      className={`flex h-full w-72 shrink-0 flex-col rounded-xl border bg-[var(--surface)] transition-colors ${
        isOver
          ? 'border-[var(--primary)] shadow-md ring-1 ring-[var(--primary-soft)]'
          : 'border-[var(--border-subtle)]'
      }`}
    >
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-3 py-2.5">
        <div className="flex items-center gap-2">
          <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-[var(--surface-hover)] px-1.5 text-[0.6875rem] font-semibold tabular-nums text-[var(--text)]">
            {rows.length}
          </span>
          <span className="text-xs font-semibold uppercase tracking-wider text-[var(--text)]">
            {localizeStatus(groupKey, locale)}
          </span>
        </div>
        <button
          type="button"
          onClick={() => onAddCard(groupKey)}
          className="rounded p-1 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-colors"
          title={t(locale, 'workspace.addCard')}
        >
          <Plus size={13} />
        </button>
      </div>
      <div className="flex-1 space-y-2 overflow-y-auto p-2 min-h-0">
        <SortableContext items={rows.map((r) => String(r.id))} strategy={verticalListSortingStrategy}>
          {rows.map((row) => (
            <SortableCard
              key={String(row.id)}
              row={row}
              groupKey={groupKey}
              groupBy={groupBy}
              onEdit={onEdit}
              onDelete={onDelete}
            />
          ))}
        </SortableContext>
        {rows.length === 0 && (
          <div className="rounded-lg border border-dashed border-[var(--border-subtle)] p-4 text-center text-xs text-[var(--text-disabled)]">
            {t(locale, 'workspace.emptyBoard')}
          </div>
        )}
      </div>
    </div>
  );
}

/** 可排序卡片：指针/键盘均可用，拖拽中由 DragOverlay 镜像渲染。 */
function SortableCard({
  row,
  groupKey,
  groupBy,
  onEdit,
  onDelete,
}: {
  row: DataRow;
  groupKey: string;
  groupBy: string;
  onEdit: (entry: { row: DataRow; groupKey: string }) => void;
  onDelete: (rowId: string) => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: String(row.id),
    data: { group: groupKey },
  });
  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };
  void groupBy;
  void onDelete;
  return (
    <div
      ref={setNodeRef}
      style={style}
      {...attributes}
      {...listeners}
      className={`group relative flex cursor-grab items-start justify-between rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] p-2.5 shadow-sm transition-all hover:border-[var(--primary)] active:cursor-grabbing ${
        isDragging ? 'opacity-40 scale-95 border-dashed border-[var(--primary)]' : 'hover:shadow-md'
      }`}
    >
      <div className="flex-1 min-w-0 pr-1">
        <p className="text-xs font-medium leading-snug text-[var(--text)]">
          {primaryText(row, ['name'])}
        </p>
        <div className="mt-2 flex items-center gap-2 text-[0.6875rem] text-[var(--text-secondary)]">
          {row.assignee && (
            <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 font-medium text-[var(--text)]">
              {String(row.assignee)}
            </span>
          )}
          {row.due && <span>{String(row.due)}</span>}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
        <button
          type="button"
          onClick={() => onEdit({ row, groupKey })}
          className="rounded p-1 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
          title={t(useLocale(), 'workspace.editCard')}
        >
          <Pencil size={12} />
        </button>
        <GripVertical size={13} className="text-[var(--text-disabled)] mt-0.5" />
      </div>
    </div>
  );
}

/** 任务增/改/删弹窗（真实双向编辑）。 */
function TaskEditorModal({
  row,
  groupKey,
  groupBy,
  groups,
  onSave,
  onDelete,
  onClose,
}: {
  row: DataRow;
  groupKey: string;
  groupBy: string;
  groups: string[];
  onSave: (patch: Partial<DataRow>) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const locale = useLocale();
  const [name, setName] = useState(String(row.name ?? ''));
  const [assignee, setAssignee] = useState(String(row.assignee ?? ''));
  const [due, setDue] = useState(String(row.due ?? ''));
  const [group, setGroup] = useState(groupKey);

  const inputClass =
    'w-full rounded-lg border border-[var(--border)] bg-[var(--surface)] px-2.5 py-1.5 text-xs text-[var(--text)] outline-none transition-colors focus:border-[var(--primary)]';

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--overlay-soft)]"
      onClick={onClose}
      role="presentation"
    >
      <div
        className="w-80 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 shadow-lg"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <div className="mb-3 flex items-center justify-between">
          <span className="text-sm font-semibold text-[var(--text)]">
            {t(locale, 'workspace.editCard')}
          </span>
          <button type="button" onClick={onClose} className="rounded p-1 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)]">
            <X size={14} />
          </button>
        </div>
        <div className="space-y-2.5">
          <label className="block">
            <span className="mb-1 block text-[0.6875rem] font-medium text-[var(--text-secondary)]">
              {t(locale, 'workspace.colName')}
            </span>
            <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </label>
          <label className="block">
            <span className="mb-1 block text-[0.6875rem] font-medium text-[var(--text-secondary)]">
              {t(locale, 'workspace.colAssignee')}
            </span>
            <input className={inputClass} value={assignee} onChange={(e) => setAssignee(e.target.value)} />
          </label>
          <label className="block">
            <span className="mb-1 block text-[0.6875rem] font-medium text-[var(--text-secondary)]">
              {t(locale, 'workspace.colDue')}
            </span>
            <input className={inputClass} value={due} onChange={(e) => setDue(e.target.value)} />
          </label>
          <label className="block">
            <span className="mb-1 block text-[0.6875rem] font-medium text-[var(--text-secondary)]">
              {t(locale, 'workspace.groupBy')}
            </span>
            <select
              className={inputClass}
              value={group}
              onChange={(e) => setGroup(e.target.value)}
            >
              {groups.map((g) => (
                <option key={g} value={g}>
                  {localizeStatus(g, locale)}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="mt-4 flex items-center justify-between gap-2">
          <button
            type="button"
            onClick={onDelete}
            className="inline-flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs font-medium text-[var(--danger)] transition-colors hover:bg-[var(--danger)]/10"
          >
            <Trash2 size={12} />
            {t(locale, 'workspace.deleteCard')}
          </button>
          <button
            type="button"
            onClick={() =>
              onSave({
                name,
                assignee,
                due,
                [groupBy]: group,
              })
            }
            className="rounded-lg bg-[var(--primary)] px-3 py-1.5 text-xs font-semibold text-[var(--primary-foreground)] transition-colors hover:opacity-90"
          >
            {t(locale, 'workspace.saveCard')}
          </button>
        </div>
      </div>
    </div>
  );
}

/** 星期表头顺序（周日→周六），文案走 i18n：workspace.weekday* 键。 */
const WEEKDAY_KEYS = [
  'workspace.weekdaySun',
  'workspace.weekdayMon',
  'workspace.weekdayTue',
  'workspace.weekdayWed',
  'workspace.weekdayThu',
  'workspace.weekdayFri',
  'workspace.weekdaySat',
] as const;

function CalendarView({ rows, field }: { rows: DataRow[]; field: string }) {
  const locale = useLocale();
  const [anchor, setAnchor] = useState(() => {
    const now = new Date();
    return new Date(now.getFullYear(), now.getMonth(), 1);
  });
  const monthLocale = locale === 'en' ? 'en' : 'zh-CN';

  const days = useMemo(() => {
    const year = anchor.getFullYear();
    const month = anchor.getMonth();
    const first = new Date(year, month, 1);
    const startPad = first.getDay();
    const last = new Date(year, month + 1, 0);
    const total = startPad + last.getDate();
    const cells: { date: Date; rows: DataRow[] }[] = [];
    for (let i = 0; i < total; i++) {
      const date = new Date(year, month, i - startPad + 1);
      const dateKey = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
      cells.push({
        date,
        rows: rows.filter((row) => String(row[field] ?? '') === dateKey),
      });
    }
    return cells;
  }, [anchor, rows, field]);

  const shiftMonth = (delta: number) =>
    setAnchor(new Date(anchor.getFullYear(), anchor.getMonth() + delta, 1));

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
        <button type="button" onClick={() => shiftMonth(-1)} className="rounded px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]">‹</button>
        <span className="min-w-28 text-center text-xs font-medium text-[var(--text)]">
          {t(
            locale,
            'workspace.calMonthYear',
            {
              month: anchor.toLocaleString(monthLocale, { month: 'long' }),
              year: String(anchor.getFullYear()),
            },
          )}
        </span>
        <button type="button" onClick={() => shiftMonth(1)} className="rounded px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]">›</button>
      </div>
      <div className="grid grid-cols-7 border-b border-[var(--border-subtle)]">
        {WEEKDAY_KEYS.map((key) => (
          <div key={key} className="px-2 py-1.5 text-center text-[0.625rem] font-medium text-[var(--text-disabled)]">
            {t(locale, key)}
          </div>
        ))}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-7 overflow-hidden">
        {days.map(({ date, rows: dayRows }) => {
          const today = new Date();
          const isToday = date.toDateString() === today.toDateString();
          const outside = date.getMonth() !== anchor.getMonth();
          return (
            <div key={date.toISOString()} className={`min-h-0 border-b border-r border-[var(--border-subtle)] p-1 ${outside ? 'bg-[var(--surface-hover)]/30' : ''}`}>
              <div className={`mb-0.5 flex h-5 w-5 items-center justify-center rounded text-[0.625rem] ${isToday ? 'bg-[var(--primary)] font-semibold text-[var(--primary-foreground)]' : 'text-[var(--text-disabled)]'}`}>
                {date.getDate()}
              </div>
              <div className="space-y-0.5">
                {dayRows.slice(0, 3).map((row) => (
                  <div key={String(row.id)} className="truncate rounded bg-[var(--primary-soft)] px-1 py-0.5 text-[0.625rem] text-[var(--primary)]">
                    {String(row.name)}
                  </div>
                ))}
                {dayRows.length > 3 && (
                  <div className="px-1 text-[0.625rem] text-[var(--text-disabled)]">
                    {t(locale, 'workspace.calMore', { count: dayRows.length - 3 })}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function MenuButton({
  label,
  icon,
  open,
  onClick,
  children,
}: {
  label: string;
  icon: React.ReactNode;
  open: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="relative">
      <button
        type="button"
        onClick={onClick}
        title={label}
        className={`inline-flex items-center gap-1 rounded-md px-2 py-1.5 text-xs transition-colors ${
          open ? 'bg-[var(--surface-hover)] text-[var(--text)]' : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
        }`}
      >
        {icon}
        <ChevronDown size={12} />
      </button>
      {open && (
        <div className="absolute right-0 top-full z-30 mt-1 min-w-36 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-1 shadow-lg">
          {children}
        </div>
      )}
    </div>
  );
}

function MenuRow({ active, label, onClick }: { active: boolean; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-[var(--surface-hover)] ${
        active ? 'text-[var(--primary)]' : 'text-[var(--text-secondary)]'
      }`}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${active ? 'bg-[var(--primary)]' : 'bg-[var(--border)]'}`} />
      {label}
    </button>
  );
}

function primaryText(row: DataRow, columns: string[]): string {
  const first = columns[0];
  return first && row[first] != null ? String(row[first]) : String(row.id ?? '');
}

function applyFilter(row: DataRow, f: DataFilter): boolean {
  const value = row[f.field];
  switch (f.op) {
    case 'eq':
      return String(value ?? '') === String(f.value);
    case 'neq':
      return String(value ?? '') !== String(f.value);
    case 'contains':
      return String(value ?? '').toLowerCase().includes(String(f.value).toLowerCase());
    case 'gt':
      return Number(value) > Number(f.value);
    case 'lt':
      return Number(value) < Number(f.value);
    default:
      return true;
  }
}

function EmptyData() {
  const locale = useLocale();
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 py-10 text-center">
      <Rows3 size={20} className="text-[var(--text-disabled)]" />
      <p className="text-xs text-[var(--text-disabled)]">{t(locale, 'workspace.emptyList')}</p>
    </div>
  );
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

function localizeStatus(status: string, locale: Locale): string {
  switch (status) {
    case 'todo':
      return t(locale, 'workspace.statusTodo');
    case 'in-progress':
      return t(locale, 'workspace.statusInProgress');
    case 'done':
      return t(locale, 'workspace.statusDone');
    default:
      return status;
  }
}
