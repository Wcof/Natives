'use client';

/**
 * One-shot subagent route assignment modal.
 * Submits via interaction.respond (never permission.respond).
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { t } from '@/i18n';
import type {
  SubagentAssignmentInteraction,
  SubagentAssignmentMode,
  SubagentRouteBinding,
} from '@/lib/assistant-protocol';

export interface AssignmentKeyOption {
  providerId: string;
  providerName: string;
  keyId: string;
  keyLabel: string;
  modelId: string;
  models: Array<{ id: string; displayName?: string }>;
}

export interface SubagentAssignmentModalProps {
  open: boolean;
  locale: string;
  interaction: SubagentAssignmentInteraction | null;
  keys: AssignmentKeyOption[];
  /** When set, this is a switch-key flow (subagent.switchRoute), not interaction.respond. */
  switchSessionId?: string | null;
  onClose: () => void;
  onConfirm: (payload: {
    mode: SubagentAssignmentMode;
    bindings: SubagentRouteBinding[];
    sessionId?: string | null;
  }) => void | Promise<void>;
}

function defaultBinding(keys: AssignmentKeyOption[]): SubagentRouteBinding | null {
  const first = keys[0];
  if (!first) return null;
  return {
    providerId: first.providerId,
    keyId: first.keyId,
    modelId: first.modelId || first.models[0]?.id || '',
  };
}

export default function SubagentAssignmentModal({
  open,
  locale,
  interaction,
  keys,
  switchSessionId = null,
  onClose,
  onConfirm,
}: SubagentAssignmentModalProps) {
  const [mode, setMode] = useState<SubagentAssignmentMode>('default');
  const [bindings, setBindings] = useState<SubagentRouteBinding[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const hasKeys = keys.length > 0;
  const isSwitch = Boolean(switchSessionId);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setSubmitting(false);
    setMode('default');
    const seed = defaultBinding(keys);
    setBindings(seed ? [seed] : []);
  }, [open, interaction?.id, switchSessionId, keys]);

  const title = isSwitch
    ? t(locale, 'assistant.subagentAssignment.switchKeyTitle')
    : t(locale, 'assistant.subagentAssignment.title');
  const reason =
    interaction?.reason ||
    (isSwitch
      ? t(locale, 'assistant.subagentAssignment.switchKeyHint')
      : t(locale, 'assistant.subagentAssignment.reason'));

  const effectiveBindings = useMemo(() => {
    if (!hasKeys) return [];
    if (mode === 'default') {
      const seed = defaultBinding(keys);
      return seed ? [seed] : [];
    }
    if (mode === 'random') {
      // All available keys form the random pool.
      return keys.map((k) => ({
        providerId: k.providerId,
        keyId: k.keyId,
        modelId: k.modelId || k.models[0]?.id || '',
      }));
    }
    return bindings.filter((b) => b.providerId && b.keyId && b.modelId);
  }, [mode, keys, bindings, hasKeys]);

  const updateBinding = useCallback(
    (index: number, patch: Partial<SubagentRouteBinding>) => {
      setBindings((prev) =>
        prev.map((b, i) => {
          if (i !== index) return b;
          const next = { ...b, ...patch };
          if (patch.providerId || patch.keyId) {
            const match = keys.find(
              (k) =>
                k.providerId === (patch.providerId ?? next.providerId) &&
                k.keyId === (patch.keyId ?? next.keyId),
            );
            if (match && !patch.modelId) {
              next.modelId = match.modelId || match.models[0]?.id || next.modelId;
            }
          }
          return next;
        }),
      );
    },
    [keys],
  );

  const handleConfirm = useCallback(async () => {
    if (submitting) return;
    if (!hasKeys || effectiveBindings.length === 0) {
      setError(t(locale, 'assistant.subagentAssignment.needBinding'));
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm({
        mode,
        bindings: effectiveBindings,
        sessionId: switchSessionId,
      });
      // Keep locked; parent closes modal on success.
    } catch (err) {
      setError(
        err instanceof Error && err.message
          ? err.message
          : t(locale, 'assistant.subagentAssignment.errorFallback'),
      );
      setSubmitting(false);
    }
  }, [
    submitting,
    hasKeys,
    effectiveBindings,
    locale,
    onConfirm,
    mode,
    switchSessionId,
  ]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      data-testid="subagent-assignment-modal"
    >
      <div className="w-full max-w-lg rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-xl">
        <div className="border-b border-[var(--border)] px-4 py-3">
          <h2 className="text-sm font-semibold text-[var(--text)]">{title}</h2>
          <p className="mt-1 text-xs text-[var(--text-secondary)]">{reason}</p>
        </div>

        <div className="space-y-3 px-4 py-3 text-sm">
          <div>
            <div className="mb-1 text-[11px] font-medium text-[var(--text-disabled)]">
              {t(locale, 'assistant.subagentAssignment.mode')}
            </div>
            <div className="flex flex-col gap-1.5">
              {(
                [
                  ['default', 'assistant.subagentAssignment.modeDefault'],
                  ['random', 'assistant.subagentAssignment.modeRandom'],
                  ['custom', 'assistant.subagentAssignment.modeCustom'],
                ] as const
              ).map(([value, key]) => {
                const disabled = !hasKeys && value !== 'default';
                return (
                  <label
                    key={value}
                    className={`flex items-center gap-2 rounded px-2 py-1.5 ${
                      disabled ? 'opacity-40' : 'hover:bg-[var(--surface-hover)]'
                    }`}
                  >
                    <input
                      type="radio"
                      name="subagent-assign-mode"
                      value={value}
                      checked={mode === value}
                      disabled={disabled || submitting}
                      onChange={() => setMode(value)}
                    />
                    <span>{t(locale, key)}</span>
                  </label>
                );
              })}
            </div>
          </div>

          {!hasKeys ? (
            <div className="rounded border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-xs text-[var(--warning)]">
              {t(locale, 'assistant.subagentAssignment.noKeys')}
            </div>
          ) : null}

          {mode === 'custom' && hasKeys ? (
            <div>
              <div className="mb-1 flex items-center justify-between text-[11px] font-medium text-[var(--text-disabled)]">
                <span>{t(locale, 'assistant.subagentAssignment.bindings')}</span>
                <button
                  type="button"
                  className="text-[var(--primary)] hover:underline"
                  disabled={submitting}
                  onClick={() => {
                    const seed = defaultBinding(keys);
                    if (seed) setBindings((prev) => [...prev, seed]);
                  }}
                >
                  {t(locale, 'assistant.subagentAssignment.addBinding')}
                </button>
              </div>
              <div className="space-y-2">
                {bindings.map((b, index) => {
                  const keyOpts = keys.filter((k) => k.providerId === b.providerId);
                  const providerIds = Array.from(new Set(keys.map((k) => k.providerId)));
                  const models =
                    keys.find((k) => k.providerId === b.providerId && k.keyId === b.keyId)
                      ?.models ?? [];
                  return (
                    <div
                      key={`${b.providerId}-${b.keyId}-${index}`}
                      className="grid grid-cols-[1fr_1fr_1fr_auto] gap-1.5"
                    >
                      <select
                        className="rounded border border-[var(--border)] bg-[var(--background)] px-2 py-1 text-xs"
                        value={b.providerId}
                        disabled={submitting}
                        onChange={(e) => {
                          const pid = e.target.value;
                          const first = keys.find((k) => k.providerId === pid);
                          updateBinding(index, {
                            providerId: pid,
                            keyId: first?.keyId ?? '',
                            modelId: first?.modelId || first?.models[0]?.id || '',
                          });
                        }}
                        aria-label={t(locale, 'assistant.subagentAssignment.provider')}
                      >
                        {providerIds.map((pid) => {
                          const name =
                            keys.find((k) => k.providerId === pid)?.providerName ?? pid;
                          return (
                            <option key={pid} value={pid}>
                              {name}
                            </option>
                          );
                        })}
                      </select>
                      <select
                        className="rounded border border-[var(--border)] bg-[var(--background)] px-2 py-1 text-xs"
                        value={b.keyId}
                        disabled={submitting}
                        onChange={(e) => updateBinding(index, { keyId: e.target.value })}
                        aria-label={t(locale, 'assistant.subagentAssignment.key')}
                      >
                        {(keyOpts.length ? keyOpts : keys).map((k) => (
                          <option key={k.keyId} value={k.keyId}>
                            {k.keyLabel || k.keyId}
                          </option>
                        ))}
                      </select>
                      <select
                        className="rounded border border-[var(--border)] bg-[var(--background)] px-2 py-1 text-xs"
                        value={b.modelId}
                        disabled={submitting}
                        onChange={(e) => updateBinding(index, { modelId: e.target.value })}
                        aria-label={t(locale, 'assistant.subagentAssignment.model')}
                      >
                        {(models.length
                          ? models
                          : [{ id: b.modelId, displayName: b.modelId }]
                        ).map((m) => (
                          <option key={m.id} value={m.id}>
                            {m.displayName || m.id}
                          </option>
                        ))}
                      </select>
                      <button
                        type="button"
                        className="rounded px-2 text-[11px] text-[var(--danger)] hover:bg-[var(--surface-hover)]"
                        disabled={submitting || bindings.length <= 1}
                        onClick={() =>
                          setBindings((prev) => prev.filter((_, i) => i !== index))
                        }
                      >
                        {t(locale, 'assistant.subagentAssignment.removeBinding')}
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}

          {error ? (
            <div role="alert" className="rounded border border-red-400/30 bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-950/20">
              {error}
            </div>
          ) : null}
        </div>

        <div className="flex justify-end gap-2 border-t border-[var(--border)] px-4 py-3">
          <button
            type="button"
            className="rounded border border-[var(--border)] px-3 py-1.5 text-xs hover:bg-[var(--surface-hover)]"
            disabled={submitting}
            onClick={onClose}
          >
            {t(locale, 'assistant.subagentAssignment.cancel')}
          </button>
          <button
            type="button"
            className="rounded bg-[var(--primary)] px-3 py-1.5 text-xs text-white disabled:opacity-50"
            disabled={submitting || !hasKeys}
            onClick={() => void handleConfirm()}
            data-testid="subagent-assignment-confirm"
          >
            {submitting
              ? t(locale, 'assistant.subagentAssignment.processing')
              : isSwitch
                ? t(locale, 'assistant.subagentAssignment.switchConfirm')
                : t(locale, 'assistant.subagentAssignment.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
