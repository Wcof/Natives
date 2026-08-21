'use client';

/**
 * DataView (C-027..C-031) — List / Table / Board / Calendar controlled views.
 *
 * - The view is fully controlled: mode, columns, sort, filter and group-by
 *   live in DataViewState and are persisted per view id (debounced).
 * - Renders synchronously from state (snapshot-first; no async gap).
 * - Dense/readable by default; only semantic tokens are used.
 */

import { useMemo, useState } from 'react';
import { ArrowDown, ArrowUp, CalendarDays, ChevronDown, Columns3, List, Rows3, SlidersHorizontal, Table2 } from 'lucide-react';
import type { DataFilter, DataViewState } from '@/lib/workspace/views/types';

export type DataRow = Record<string, string | number | boolean | null>;

export interface DataViewProps {
  viewId: string;
  state: DataViewState;
  onStateChange: (patch: Partial<DataViewState>) => void;
  rows: DataRow[];
}

const DEFAULT_ROWS: DataRow[] = [
  { id: 't1', name: 'Draft V2 workspace spec', status: 'todo', assignee: 'You', due: '2026-01-12' },
  { id: 't2', name: 'Wire CompactGrid keyboard layer', status: 'in-progress', assignee: 'You', due: '2026-01-14' },
  { id: 't3', name: 'Free Canvas marquee select', status: 'done', assignee: 'B', due: '2026-01-08' },
  { id: 't4', name: 'Inspector host generalization', status: 'in-progress', assignee: 'C', due: '2026-01-15' },
  { id: 't5', name: 'Snapshot-first hydration', status: 'done', assignee: 'C', due: '2026-01-06' },
  { id: 't6', name: 'Data view calendar field', status: 'todo', assignee: 'A', due: '2026-01-20' },
];

export default function DataView({ viewId, state, onStateChange, rows = DEFAULT_ROWS }: DataViewProps) {
  const [menuOpen, setMenuOpen] = useState<'columns' | 'sort' | 'filter' | null>(null);

  const filtered = useMemo(() => {
    let out = rows;
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
  }, [rows, state.filters, state.sort]);

  const groups = useMemo(() => {
    const by = state.groupBy;
    if (!by) return [{ key: 'All', items: filtered }];
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
      title={`Switch to ${label} view`}
    >
      {icon}
      <span className="hidden sm:inline">{label}</span>
    </button>
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden" data-testid={`data-view-${viewId}`}>
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-[var(--border-subtle)] px-2">
        {modeButton('list', 'List', <List size={14} />)}
        {modeButton('table', 'Table', <Table2 size={14} />)}
        {modeButton('board', 'Board', <Columns3 size={14} />)}
        {modeButton('calendar', 'Calendar', <CalendarDays size={14} />)}

        <div className="ml-auto flex items-center gap-1">
          <MenuButton label={`Sort${state.sort ? `: ${state.sort.field}` : ''}`} icon={state.sort?.dir === 'desc' ? <ArrowDown size={13} /> : <ArrowUp size={13} />} open={menuOpen === 'sort'} onClick={() => toggleMenu('sort')}>
            {state.columns.map((col) => (
              <MenuRow
                key={col}
                active={state.sort?.field === col}
                label={col}
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
          <MenuButton label="Columns" icon={<Columns3 size={13} />} open={menuOpen === 'columns'} onClick={() => toggleMenu('columns')}>
            {state.columns.map((col) => (
              <MenuRow
                key={col}
                active={!state.hiddenColumns.includes(col)}
                label={col}
                onClick={() =>
                  onStateChange({
                    hiddenColumns: state.hiddenColumns.includes(col)
                      ? state.hiddenColumns.filter((c) => c !== col)
                      : [...state.hiddenColumns, col],
                  })
                }
              />
            ))}
          </MenuButton>
          <MenuButton label="Group" icon={<SlidersHorizontal size={13} />} open={menuOpen === 'filter'} onClick={() => toggleMenu('filter')}>
            <button
              type="button"
              onClick={() => onStateChange({ groupBy: null })}
              className={`w-full rounded px-2 py-1 text-left text-xs hover:bg-[var(--surface-hover)] ${!state.groupBy ? 'text-[var(--primary)]' : 'text-[var(--text-secondary)]'}`}
            >
              None
            </button>
            {state.columns.map((col) => (
              <button
                key={col}
                type="button"
                onClick={() => onStateChange({ groupBy: state.groupBy === col ? null : col })}
                className={`w-full rounded px-2 py-1 text-left text-xs hover:bg-[var(--surface-hover)] ${state.groupBy === col ? 'text-[var(--primary)]' : 'text-[var(--text-secondary)]'}`}
              >
                {col}
              </button>
            ))}
          </MenuButton>
          <span className="px-2 text-[0.625rem] tabular-nums text-[var(--text-disabled)]">
            {filtered.length} / {rows.length}
          </span>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto bg-[var(--surface)]">
        {state.mode === 'list' && <ListView groups={groups} columns={visibleColumns} />}
        {state.mode === 'table' && <TableView rows={filtered} columns={visibleColumns} sort={state.sort} onSort={(col) => onStateChange({ sort: { field: col, dir: state.sort?.field === col && state.sort.dir === 'asc' ? 'desc' : 'asc' } })} />}
        {state.mode === 'board' && <BoardView groups={groups} />}
        {state.mode === 'calendar' && <CalendarView rows={filtered} field={state.calendarField ?? 'due'} />}
      </div>
    </div>
  );
}

function ListView({ groups, columns }: { groups: { key: string; items: DataRow[] }[]; columns: string[] }) {
  return (
    <div className="divide-y divide-[var(--border-subtle)]">
      {groups.map((group) => (
        <section key={group.key} className="px-3 py-2">
          <h3 className="mb-1 flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
            <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-[0.625rem] tabular-nums text-[var(--text)]">
              {group.items.length}
            </span>
            {group.key}
          </h3>
          <ul className="space-y-1">
            {group.items.map((row) => (
              <li key={String(row.id)} className="flex items-center gap-2 rounded-lg border border-[var(--border-subtle)] px-3 py-2 text-xs">
                <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--primary)]/70" />
                <span className="min-w-0 flex-1 truncate text-[var(--text)]">{primaryText(row, columns)}</span>
                {columns.slice(1, 3).map((col) => (
                  <span key={col} className="shrink-0 rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-[0.625rem] text-[var(--text-secondary)]">
                    {String(row[col] ?? '—')}
                  </span>
                ))}
              </li>
            ))}
          </ul>
        </section>
      ))}
      {groups.length === 0 && <EmptyData />}
    </div>
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
  sort: DataViewState['sort'];
  onSort: (col: string) => void;
}) {
  return (
    <table className="w-full border-collapse text-xs">
      <thead className="sticky top-0 bg-[var(--surface-hover)]">
        <tr>
          {columns.map((col) => (
            <th key={col} className="border-b border-[var(--border-subtle)] px-3 py-2 text-left font-medium text-[var(--text-secondary)]">
              <button type="button" onClick={() => onSort(col)} className="inline-flex items-center gap-1 hover:text-[var(--text)]">
                {col}
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

function BoardView({ groups }: { groups: { key: string; items: DataRow[] }[] }) {
  return (
    <div className="flex h-full items-start gap-3 overflow-x-auto p-3">
      {groups.map((group) => (
        <div key={group.key} className="flex h-full w-64 shrink-0 flex-col rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)]">
          <div className="flex items-center gap-2 border-b border-[var(--border-subtle)] px-3 py-2">
            <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 text-[0.625rem] tabular-nums text-[var(--text)]">
              {group.items.length}
            </span>
            <span className="text-xs font-medium text-[var(--text)]">{group.key}</span>
          </div>
          <div className="flex-1 space-y-2 overflow-y-auto p-2">
            {group.items.map((row) => (
              <div key={String(row.id)} className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface)] p-2.5 shadow-sm">
                <p className="text-xs font-medium leading-snug text-[var(--text)]">{primaryText(row, ['name'])}</p>
                <div className="mt-1.5 flex items-center gap-1.5 text-[0.625rem] text-[var(--text-disabled)]">
                  {row.assignee ? <span>{String(row.assignee)}</span> : null}
                  {row.due ? <span>· {String(row.due)}</span> : null}
                </div>
              </div>
            ))}
            {group.items.length === 0 && (
              <div className="rounded-lg border border-dashed border-[var(--border-subtle)] p-2 text-center text-[0.625rem] text-[var(--text-disabled)]">
                Empty
              </div>
            )}
          </div>
        </div>
      ))}
      {groups.length === 0 && <EmptyData />}
    </div>
  );
}

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function CalendarView({ rows, field }: { rows: DataRow[]; field: string }) {
  const [anchor, setAnchor] = useState(() => {
    const now = new Date();
    return new Date(now.getFullYear(), now.getMonth(), 1);
  });

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
          {anchor.toLocaleString('en', { month: 'long', year: 'numeric' })}
        </span>
        <button type="button" onClick={() => shiftMonth(1)} className="rounded px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]">›</button>
      </div>
      <div className="grid grid-cols-7 border-b border-[var(--border-subtle)]">
        {WEEKDAYS.map((day) => (
          <div key={day} className="px-2 py-1.5 text-center text-[0.625rem] font-medium text-[var(--text-disabled)]">
            {day}
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
                  <div className="px-1 text-[0.625rem] text-[var(--text-disabled)]">+{dayRows.length - 3} more</div>
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
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 py-10 text-center">
      <Rows3 size={20} className="text-[var(--text-disabled)]" />
      <p className="text-xs text-[var(--text-disabled)]">No rows match the current filters.</p>
    </div>
  );
}
