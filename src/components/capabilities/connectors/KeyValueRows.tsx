'use client';

import { Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';

export interface KvRow {
  key: string;
  value: string;
  isSecretRef: boolean;
}

interface KeyValueRowsProps {
  locale: Locale;
  rows: KvRow[];
  onRows: (rows: KvRow[]) => void;
  onTouched: () => void;
  /** Show the secret-reference checkbox per row (env vars only). */
  withSecret: boolean;
  keyLabel: string;
  valueLabel: string;
  addLabel: string;
}

const inputStyle = {
  borderColor: 'var(--border)',
  background: 'var(--surface)',
  color: 'var(--text)',
} as const;

/** Editable key/value rows for connector env vars and headers. */
export default function KeyValueRows({
  locale,
  rows,
  onRows,
  onTouched,
  withSecret,
  keyLabel,
  valueLabel,
  addLabel,
}: KeyValueRowsProps) {
  return (
    <div className="space-y-1.5">
      {rows.map((row, index) => (
        <div key={index} className="flex items-center gap-1.5">
          <input
            value={row.key}
            onChange={(e) => {
              onTouched();
              onRows(rows.map((r, i) => (i === index ? { ...r, key: e.target.value } : r)));
            }}
            placeholder={keyLabel}
            aria-label={keyLabel}
            className="w-2/5 rounded border px-2 py-1.5 font-mono text-xs"
            style={inputStyle}
          />
          <input
            value={row.value}
            onChange={(e) => {
              onTouched();
              onRows(rows.map((r, i) => (i === index ? { ...r, value: e.target.value } : r)));
            }}
            placeholder={row.isSecretRef ? 'secret:<id>' : valueLabel}
            aria-label={valueLabel}
            className="flex-1 rounded border px-2 py-1.5 font-mono text-xs"
            style={inputStyle}
          />
          {withSecret ? (
            <label
              className="flex shrink-0 items-center gap-1 text-[10px]"
              style={{ color: 'var(--text-secondary)' }}
              title={t(locale, 'capabilities.connectors.secretRefHint')}
            >
              <input
                type="checkbox"
                checked={row.isSecretRef}
                onChange={(e) => {
                  onTouched();
                  onRows(rows.map((r, i) => (i === index ? { ...r, isSecretRef: e.target.checked } : r)));
                }}
              />
              {t(locale, 'capabilities.connectors.secretRef')}
            </label>
          ) : null}
          <button
            type="button"
            onClick={() => {
              onTouched();
              onRows(rows.filter((_, i) => i !== index));
            }}
            aria-label={t(locale, 'capabilities.connectors.remove')}
            title={t(locale, 'capabilities.connectors.remove')}
            className="rounded p-1 hover:bg-[var(--surface-hover)]"
            style={{ color: 'var(--text-disabled)' }}
          >
            <Trash2 size={12} />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() => onRows([...rows, { key: '', value: '', isSecretRef: false }])}
        className="text-xs"
        style={{ color: 'var(--primary)' }}
      >
        + {addLabel}
      </button>
    </div>
  );
}
