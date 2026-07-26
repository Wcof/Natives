'use client';

import { useState } from 'react';
import { Eye, EyeOff, Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { CapabilitySecretEntry } from '@/lib/assistant-workspace/capability-secrets';

export interface KvRow {
  key: string;
  value: string;
  isSecretRef: boolean;
  /** Hydrated from a stored server — the daemon never echoes the value back. */
  stored: boolean;
  /** Value/secret-ref edited by the user in this dialog session. */
  touched: boolean;
}

export const newKvRow = (partial?: Partial<KvRow>): KvRow => ({
  key: '',
  value: '',
  isSecretRef: false,
  stored: false,
  touched: false,
  ...partial,
});

/**
 * Build the update payload map. Stored rows the user never touched are sent as
 * `null` — the daemon-side sentinel for "keep the stored value" (values are
 * never echoed to the UI, so they cannot be sent back verbatim). Removed rows
 * are simply absent, which the daemon's whole-map-replace semantics treats as
 * deletion. Create dialogs only ever contain non-stored rows, so `null` never
 * appears in a create payload.
 */
export function collectKvRows(rows: KvRow[]): Record<string, string | null> {
  const out: Record<string, string | null> = {};
  for (const row of rows) {
    const key = row.key.trim();
    if (!key) continue;
    out[key] = row.stored && !row.touched ? null : row.value;
  }
  return out;
}

interface KeyValueRowsProps {
  locale: Locale;
  rows: KvRow[];
  onRows: (rows: KvRow[]) => void;
  onTouched: () => void;
  /** Show the secret-reference checkbox per row (env vars only). */
  withSecret: boolean;
  /** Saved secrets for the reference picker (edit mode only). */
  secrets?: CapabilitySecretEntry[];
  /** Create a secret and return its id (edit mode only); absent → show save-first hint. */
  onCreateSecret?: (keyName: string, plaintext: string) => Promise<string>;
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
  secrets,
  onCreateSecret,
  keyLabel,
  valueLabel,
  addLabel,
}: KeyValueRowsProps) {
  const [revealed, setRevealed] = useState<ReadonlySet<number>>(new Set());
  const [creatingIndex, setCreatingIndex] = useState<number | null>(null);
  const [secretName, setSecretName] = useState('');
  const [secretPlain, setSecretPlain] = useState('');
  const [savingSecret, setSavingSecret] = useState(false);
  const [secretError, setSecretError] = useState<string | null>(null);

  const patchRow = (index: number, patch: Partial<KvRow>) => {
    onTouched();
    onRows(rows.map((r, i) => (i === index ? { ...r, ...patch, touched: true } : r)));
  };

  const toggleReveal = (index: number) => {
    setRevealed((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  const openCreate = (index: number) => {
    setCreatingIndex(index);
    setSecretName(rows[index]?.key.trim() ?? '');
    setSecretPlain('');
    setSecretError(null);
  };

  const handleCreateSecret = async (index: number) => {
    if (!onCreateSecret || savingSecret) return;
    if (!secretName.trim() || !secretPlain) return;
    setSavingSecret(true);
    setSecretError(null);
    try {
      const id = await onCreateSecret(secretName.trim(), secretPlain);
      patchRow(index, { value: `secret:${id}` });
      setCreatingIndex(null);
      setSecretPlain('');
    } catch (e) {
      setSecretError(classifyError(e).userMessage);
    } finally {
      setSavingSecret(false);
    }
  };

  return (
    <div className="space-y-1.5">
      {rows.map((row, index) => {
        const showPlain = row.isSecretRef || revealed.has(index);
        const untouchedStored = row.stored && !row.touched;
        return (
          <div key={index}>
            <div className="flex items-center gap-1.5">
              <input
                value={row.key}
                readOnly={row.stored}
                title={row.stored ? t(locale, 'capabilities.connectors.keyReadOnlyHint') : undefined}
                onChange={(e) => {
                  if (row.stored) return;
                  patchRow(index, { key: e.target.value });
                }}
                placeholder={keyLabel}
                aria-label={keyLabel}
                className="w-2/5 rounded border px-2 py-1.5 font-mono text-xs read-only:opacity-60"
                style={inputStyle}
              />
              <input
                type={showPlain ? 'text' : 'password'}
                value={row.value}
                onChange={(e) => patchRow(index, { value: e.target.value })}
                placeholder={
                  row.isSecretRef
                    ? 'secret:<id>'
                    : untouchedStored
                      ? t(locale, 'capabilities.connectors.valueStoredPlaceholder')
                      : valueLabel
                }
                aria-label={valueLabel}
                className="flex-1 rounded border px-2 py-1.5 font-mono text-xs"
                style={inputStyle}
              />
              {!row.isSecretRef ? (
                <button
                  type="button"
                  onClick={() => toggleReveal(index)}
                  aria-label={t(locale, revealed.has(index) ? 'capabilities.connectors.hideValue' : 'capabilities.connectors.revealValue')}
                  title={t(locale, revealed.has(index) ? 'capabilities.connectors.hideValue' : 'capabilities.connectors.revealValue')}
                  className="rounded p-1 hover:bg-[var(--surface-hover)]"
                  style={{ color: 'var(--text-disabled)' }}
                >
                  {revealed.has(index) ? <EyeOff size={12} /> : <Eye size={12} />}
                </button>
              ) : null}
              {withSecret ? (
                <label
                  className="flex shrink-0 items-center gap-1 text-[10px]"
                  style={{ color: 'var(--text-secondary)' }}
                  title={t(locale, 'capabilities.connectors.secretRefHint')}
                >
                  <input
                    type="checkbox"
                    checked={row.isSecretRef}
                    onChange={(e) => patchRow(index, { isSecretRef: e.target.checked })}
                  />
                  {t(locale, 'capabilities.connectors.secretRef')}
                </label>
              ) : null}
              <button
                type="button"
                onClick={() => {
                  onTouched();
                  setCreatingIndex(null);
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

            {withSecret && row.isSecretRef ? (
              onCreateSecret ? (
                <div className="mt-1 pl-[41.5%]">
                  {creatingIndex === index ? (
                    <div className="space-y-1">
                      <div className="flex items-center gap-1.5">
                        <input
                          value={secretName}
                          onChange={(e) => setSecretName(e.target.value)}
                          placeholder={t(locale, 'capabilities.connectors.secretName')}
                          aria-label={t(locale, 'capabilities.connectors.secretName')}
                          className="w-1/3 rounded border px-2 py-1.5 font-mono text-xs"
                          style={inputStyle}
                        />
                        <input
                          type="password"
                          value={secretPlain}
                          onChange={(e) => setSecretPlain(e.target.value)}
                          placeholder={t(locale, 'capabilities.connectors.secretValue')}
                          aria-label={t(locale, 'capabilities.connectors.secretValue')}
                          className="flex-1 rounded border px-2 py-1.5 font-mono text-xs"
                          style={inputStyle}
                        />
                        <button
                          type="button"
                          onClick={() => void handleCreateSecret(index)}
                          disabled={savingSecret || !secretName.trim() || !secretPlain}
                          className="btn btn-primary px-2 py-1 text-xs disabled:opacity-50"
                        >
                          {t(locale, 'capabilities.connectors.secretCreate')}
                        </button>
                        <button
                          type="button"
                          onClick={() => setCreatingIndex(null)}
                          className="px-2 py-1 text-xs"
                          style={{ color: 'var(--text-secondary)' }}
                        >
                          {t(locale, 'capabilities.common.cancel')}
                        </button>
                      </div>
                      {secretError ? (
                        <p className="text-xs" style={{ color: 'var(--danger)' }}>{secretError}</p>
                      ) : null}
                    </div>
                  ) : (
                    <div className="flex items-center gap-1.5">
                      <select
                        value=""
                        onChange={(e) => {
                          if (e.target.value) patchRow(index, { value: `secret:${e.target.value}` });
                        }}
                        aria-label={t(locale, 'capabilities.connectors.secretPick')}
                        className="rounded border px-2 py-1 text-xs"
                        style={inputStyle}
                      >
                        <option value="">{t(locale, 'capabilities.connectors.secretPick')}</option>
                        {(secrets ?? []).map((s) => (
                          <option key={s.id} value={s.id}>
                            {(s.keyName ?? s.id) + ' · ' + s.kind}
                          </option>
                        ))}
                      </select>
                      <button
                        type="button"
                        onClick={() => openCreate(index)}
                        className="text-xs"
                        style={{ color: 'var(--primary)' }}
                      >
                        + {t(locale, 'capabilities.connectors.secretNew')}
                      </button>
                    </div>
                  )}
                </div>
              ) : (
                <p className="mt-1 pl-[41.5%] text-[10px]" style={{ color: 'var(--text-secondary)' }}>
                  {t(locale, 'capabilities.connectors.secretCreateFirstSave')}
                </p>
              )
            ) : null}
          </div>
        );
      })}
      <button
        type="button"
        onClick={() => onRows([...rows, newKvRow()])}
        className="text-xs"
        style={{ color: 'var(--primary)' }}
      >
        + {addLabel}
      </button>
    </div>
  );
}
