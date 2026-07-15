'use client';

import React from 'react';
import { SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t } from '@/i18n';
import type { UsageDimension } from '@/types/usage';
import { Terminal, Cpu, Folder, Monitor, ChevronDown } from 'lucide-react';

interface Props {
  preset: string;
  onPresetChange: (preset: string) => void;
  customStart: string;
  customEnd: string;
  onCustomStartChange: (val: string) => void;
  onCustomEndChange: (val: string) => void;
  sources: UsageDimension[];
  models: UsageDimension[];
  projects: UsageDimension[];
  terminals: UsageDimension[];
  sourceFilter: string[] | null;
  modelFilter: string[] | null;
  projectFilter: string[] | null;
  terminalFilter: string[] | null;
  onSourceFilterChange: (val: string[] | null) => void;
  onModelFilterChange: (val: string[] | null) => void;
  onProjectFilterChange: (val: string[] | null) => void;
  onTerminalFilterChange: (val: string[] | null) => void;
  onSelectDir?: () => void;
  style?: React.CSSProperties;
}

const PRESETS = [
  { key: 'today', labelKey: 'usage.dateToday' },
  { key: '24h', labelKey: 'usage.date24h' },
  { key: '7d', labelKey: 'usage.date7d' },
  { key: '30d', labelKey: 'usage.date30d' },
  { key: '90d', labelKey: 'usage.date90d' },
  { key: 'custom', labelKey: 'usage.dateCustom' },
];

interface FilterDropdownProps {
  icon: React.ReactNode;
  value: string;
  onChange: (val: string) => void;
  options: { id: string; label: string }[];
  placeholder: string;
}

function FilterDropdown({ icon, value, onChange, options, placeholder }: FilterDropdownProps) {
  return (
    <div style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
      <span style={{ position: 'absolute', left: 10, pointerEvents: 'none', display: 'flex', alignItems: 'center', color: 'var(--text-secondary)' }}>
        {icon}
      </span>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        style={{
          padding: '5px 24px 5px 28px',
          borderRadius: 20,
          border: '1px solid var(--border)',
          background: 'var(--control-bg)',
          color: 'var(--control-fg)',
          fontSize: '11px',
          cursor: 'pointer',
          appearance: 'none',
          WebkitAppearance: 'none',
          fontFamily: 'inherit',
          outline: 'none',
          fontWeight: 500,
        }}
      >
        <option value="">{placeholder}</option>
        {options.map((o) => (
          <option key={o.id} value={o.id}>
            {o.label}
          </option>
        ))}
      </select>
      <span style={{ position: 'absolute', right: 10, pointerEvents: 'none', display: 'flex', alignItems: 'center', color: 'var(--text-secondary)' }}>
        <ChevronDown size={10} />
      </span>
    </div>
  );
}

export function UsageToolbar({
  preset, onPresetChange, customStart, customEnd, onCustomStartChange, onCustomEndChange,
  sources, models, projects, terminals,
  sourceFilter, modelFilter, projectFilter, terminalFilter,
  onSourceFilterChange, onModelFilterChange, onProjectFilterChange, onTerminalFilterChange,
  onSelectDir,
  style,
}: Props) {
  const locale = useLocale();

  return (
    <div style={{
      marginBottom: SPACING.md,
      display: 'flex',
      alignItems: 'center',
      gap: SPACING.sm,
      flexWrap: 'wrap',
      justifyContent: 'flex-start',
      ...style
    }}>
      {/* ── Date presets pill container ── */}
      <div style={{
        display: 'flex',
        background: 'var(--control-bg)',
        padding: '3px',
        borderRadius: 20,
        border: '1px solid var(--border)',
        alignItems: 'center',
      }}>
        {PRESETS.map((p) => {
          const active = preset === p.key;
          const label = t(locale, p.labelKey);
          return (
            <button
              key={p.key}
              onClick={() => onPresetChange(p.key)}
              style={{
                padding: '4px 12px',
                borderRadius: 16,
                fontSize: '11px',
                border: 'none',
                background: active ? 'var(--control-selected-bg)' : 'transparent',
                color: active ? 'var(--control-selected-fg)' : 'var(--control-fg)',
                cursor: 'pointer',
                fontWeight: active ? 600 : 400,
                transition: 'all 0.1s ease',
              }}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* Custom range date picker */}
      {preset === 'custom' && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <input
            type="date"
            value={customStart}
            onChange={(e) => onCustomStartChange(e.target.value)}
            style={{
              padding: '4px 8px',
              borderRadius: BORDER_RADIUS.sm,
              border: '1px solid var(--border)',
              background: 'var(--control-bg)',
              color: 'var(--text)',
              fontSize: '11px',
              outline: 'none',
            }}
          />
          <span style={{ fontSize: '11px', color: 'var(--text-secondary)' }}>—</span>
          <input
            type="date"
            value={customEnd}
            onChange={(e) => onCustomEndChange(e.target.value)}
            style={{
              padding: '4px 8px',
              borderRadius: BORDER_RADIUS.sm,
              border: '1px solid var(--border)',
              background: 'var(--control-bg)',
              color: 'var(--text)',
              fontSize: '11px',
              outline: 'none',
            }}
          />
        </div>
      )}

      {/* ── Filters with Icons ── */}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
        {sources.length > 0 && (
          <FilterDropdown
            icon={<Terminal size={12} />}
            value={sourceFilter?.[0] ?? ''}
            onChange={(val) => onSourceFilterChange(val ? [val] : null)}
            options={sources}
            placeholder={t(locale, 'usage.filterTool') + ' ' + t(locale, 'usage.sourceAll')}
          />
        )}

        {models.length > 0 && (
          <FilterDropdown
            icon={<Cpu size={12} />}
            value={modelFilter?.[0] ?? ''}
            onChange={(val) => onModelFilterChange(val ? [val] : null)}
            options={models}
            placeholder={t(locale, 'usage.filterModel') + ' ' + t(locale, 'usage.modelAll')}
          />
        )}

        {projects.length > 0 && (
          <FilterDropdown
            icon={<Folder size={12} />}
            value={projectFilter?.[0] ?? ''}
            onChange={(val) => {
              if (val === '__select_dir__') {
                onSelectDir?.();
              } else {
                onProjectFilterChange(val ? [val] : null);
              }
            }}
            options={[
              ...projects,
              { id: '__select_dir__', label: t(locale, 'usage.selectDir') },
            ]}
            placeholder={t(locale, 'usage.filterProject') + ' ' + t(locale, 'usage.projectAll')}
          />
        )}

        {terminals.length > 0 && (
          <FilterDropdown
            icon={<Monitor size={12} />}
            value={terminalFilter?.[0] ?? ''}
            onChange={(val) => onTerminalFilterChange(val ? [val] : null)}
            options={terminals}
            placeholder={t(locale, 'usage.filterTerminal') + ' ' + t(locale, 'usage.terminalAll')}
          />
        )}
      </div>
    </div>
  );
}
