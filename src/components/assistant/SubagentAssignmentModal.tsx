'use client';

/**
 * One-shot subagent route assignment modal.
 * Submits via interaction.respond (never permission.respond).
 *
 * Modes:
 * - default: strictly use payload.default_binding for every task
 * - random: pool of isActive && status==='valid' keys
 * - custom: one fixed row per task (no add/remove), each maps to a binding by call_id
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { t } from '@/i18n';
import type {
  SubagentAssignmentInteraction,
  SubagentAssignmentMode,
  SubagentAssignmentTask,
  SubagentRouteBinding,
} from '@/lib/assistant-protocol';

export interface AssignmentKeyOption {
  providerId: string;
  providerName: string;
  keyId: string;
  keyLabel: string;
  modelId: string;
  models: Array<{ id: string; displayName?: string }>;
  isActive?: boolean;
  status?: 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable' | string;
}

export interface SubagentAssignmentConfirmPayload {
  mode: SubagentAssignmentMode;
  /** Per-task assignments (call_id → binding). Always filled for default/custom. */
  assignments: Array<{
    callId: string;
    providerId: string;
    keyId: string;
    modelId: string;
  }>;
  /** Random-mode pool of valid keys (empty for other modes). */
  pool: Array<{ providerId: string; keyId: string; modelId: string }>;
  /** Flat bindings for legacy daemon path (policy upsert). */
  bindings: SubagentRouteBinding[];
  sessionId?: string | null;
}

export interface SubagentAssignmentModalProps {
  open: boolean;
  locale: string;
  interaction: SubagentAssignmentInteraction | null;
  keys: AssignmentKeyOption[];
  /** When set, this is a switch-key flow (subagent.switchRoute), not interaction.respond. */
  switchSessionId?: string | null;
  onClose: () => void;
  onConfirm: (payload: SubagentAssignmentConfirmPayload) => void | Promise<void>;
}

function isValidKey(k: AssignmentKeyOption): boolean {
  if (k.isActive === false) return false;
  // Prefer explicit valid status; if status is absent (gateway fallback), treat as usable.
  if (k.status != null && k.status !== 'valid') return false;
  return Boolean(k.providerId && k.keyId);
}

function bindingFromKey(k: AssignmentKeyOption): SubagentRouteBinding {
  return {
    providerId: k.providerId,
    keyId: k.keyId,
    modelId: k.modelId || k.models[0]?.id || '',
  };
}

function taskList(interaction: SubagentAssignmentInteraction | null): SubagentAssignmentTask[] {
  const tasks = interaction?.tasks ?? [];
  if (tasks.length > 0) {
    return tasks.map((task, index) => ({
      callId: task.callId || `task-${index}`,
      name: task.name || task.prompt || `Task ${index + 1}`,
      prompt: task.prompt ?? null,
    }));
  }
  // Switch-key or empty payload: single synthetic row.
  return [{ callId: 'switch', name: interaction?.reason || 'Subagent', prompt: null }];
}

function seedBindingsForTasks(
  tasks: SubagentAssignmentTask[],
  validKeys: AssignmentKeyOption[],
  defaultBinding: SubagentRouteBinding | null | undefined,
): SubagentRouteBinding[] {
  const seed =
    defaultBinding && defaultBinding.providerId && defaultBinding.keyId && defaultBinding.modelId
      ? defaultBinding
      : validKeys[0]
        ? bindingFromKey(validKeys[0])
        : null;
  if (!seed) return tasks.map(() => ({ providerId: '', keyId: '', modelId: '' }));
  return tasks.map(() => ({ ...seed }));
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

  const isSwitch = Boolean(switchSessionId);
  const tasks = useMemo(() => taskList(interaction), [interaction]);
  const defaultBinding = interaction?.defaultBinding ?? null;
  const hasDefaultBinding = Boolean(
    defaultBinding?.providerId && defaultBinding?.keyId && defaultBinding?.modelId,
  );

  // Random / custom only allow active+valid keys.
  const validKeys = useMemo(() => keys.filter(isValidKey), [keys]);
  const hasValidKeys = validKeys.length > 0;

  useEffect(() => {
    if (!open) return;
    setError(null);
    setSubmitting(false);
    setMode('default');
    setBindings(seedBindingsForTasks(tasks, validKeys, defaultBinding));
  }, [open, interaction?.id, switchSessionId, tasks, validKeys, defaultBinding]);

  const title = isSwitch
    ? t(locale, 'assistant.subagentAssignment.switchKeyTitle')
    : t(locale, 'assistant.subagentAssignment.title');
  const reason =
    interaction?.reason ||
    (isSwitch
      ? t(locale, 'assistant.subagentAssignment.switchKeyHint')
      : t(locale, 'assistant.subagentAssignment.reason'));

  const updateBinding = useCallback(
    (index: number, patch: Partial<SubagentRouteBinding>) => {
      setBindings((prev) =>
        prev.map((b, i) => {
          if (i !== index) return b;
          const next = { ...b, ...patch };
          if (patch.providerId || patch.keyId) {
            const match = validKeys.find(
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
    [validKeys],
  );

  const confirmDisabled = useMemo(() => {
    if (submitting) return true;
    if (mode === 'default') {
      // Switch flow may not carry default_binding — fall back to first valid key.
      if (isSwitch) return !hasDefaultBinding && !hasValidKeys;
      return !hasDefaultBinding;
    }
    if (mode === 'random') return !hasValidKeys;
    // custom: every task row must be complete
    if (bindings.length !== tasks.length) return true;
    return bindings.some((b) => !b.providerId || !b.keyId || !b.modelId);
  }, [
    submitting,
    mode,
    isSwitch,
    hasDefaultBinding,
    hasValidKeys,
    bindings,
    tasks.length,
  ]);

  const handleConfirm = useCallback(async () => {
    if (submitting || confirmDisabled) return;

    let assignments: SubagentAssignmentConfirmPayload['assignments'] = [];
    let pool: SubagentAssignmentConfirmPayload['pool'] = [];
    let flatBindings: SubagentRouteBinding[] = [];

    if (mode === 'default') {
      const seed =
        hasDefaultBinding && defaultBinding
          ? defaultBinding
          : validKeys[0]
            ? bindingFromKey(validKeys[0])
            : null;
      if (!seed) {
        setError(t(locale, 'assistant.subagentAssignment.missingDefaultBinding'));
        return;
      }
      assignments = tasks.map((task) => ({
        callId: task.callId,
        providerId: seed.providerId,
        keyId: seed.keyId,
        modelId: seed.modelId,
      }));
      flatBindings = [seed];
    } else if (mode === 'random') {
      if (!hasValidKeys) {
        setError(t(locale, 'assistant.subagentAssignment.needValidKey'));
        return;
      }
      pool = validKeys.map((k) => bindingFromKey(k));
      flatBindings = pool;
      // Still emit assignments (daemon may ignore) using first pool entry per task.
      const first = pool[0]!;
      assignments = tasks.map((task) => ({
        callId: task.callId,
        providerId: first.providerId,
        keyId: first.keyId,
        modelId: first.modelId,
      }));
    } else {
      // custom — fixed rows, one per task
      if (bindings.length !== tasks.length || bindings.some((b) => !b.providerId || !b.keyId || !b.modelId)) {
        setError(t(locale, 'assistant.subagentAssignment.needBinding'));
        return;
      }
      assignments = tasks.map((task, i) => ({
        callId: task.callId,
        providerId: bindings[i]!.providerId,
        keyId: bindings[i]!.keyId,
        modelId: bindings[i]!.modelId,
      }));
      flatBindings = bindings.map((b) => ({ ...b }));
    }

    setSubmitting(true);
    setError(null);
    try {
      await onConfirm({
        mode,
        assignments,
        pool,
        bindings: flatBindings,
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
    confirmDisabled,
    mode,
    hasDefaultBinding,
    defaultBinding,
    validKeys,
    hasValidKeys,
    tasks,
    bindings,
    locale,
    onConfirm,
    switchSessionId,
  ]);

  if (!open) return null;

  const defaultSummary = hasDefaultBinding && defaultBinding
    ? `${defaultBinding.providerId} · ${defaultBinding.keyId.slice(0, 8)} · ${defaultBinding.modelId}`
    : null;

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
          {/* Task list — always show full batch */}
          {!isSwitch && tasks.length > 0 ? (
            <div>
              <div className="mb-1 text-[11px] font-medium text-[var(--text-disabled)]">
                {t(locale, 'assistant.subagentAssignment.tasks')}
              </div>
              <ul
                className="max-h-28 space-y-1 overflow-y-auto rounded border border-[var(--border)] bg-[var(--background)] px-2 py-1.5"
                data-testid="assignment-task-list"
              >
                {tasks.map((task) => (
                  <li
                    key={task.callId}
                    className="truncate text-xs text-[var(--text-secondary)]"
                    title={task.prompt || task.name}
                  >
                    <span className="font-medium text-[var(--text)]">{task.name}</span>
                    {task.prompt && task.prompt !== task.name ? (
                      <span className="text-[var(--text-disabled)]"> — {task.prompt}</span>
                    ) : null}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}

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
                const disabled =
                  (value === 'default' && !hasDefaultBinding && !isSwitch) ||
                  ((value === 'random' || value === 'custom') && !hasValidKeys);
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

          {mode === 'default' ? (
            hasDefaultBinding && defaultSummary ? (
              <div
                className="rounded border border-[var(--border)] bg-[var(--background)] px-3 py-2 text-xs text-[var(--text-secondary)]"
                data-testid="assignment-default-binding"
              >
                {t(locale, 'assistant.subagentAssignment.defaultBindingLabel')}: {defaultSummary}
              </div>
            ) : (
              <div
                className="rounded border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-xs text-[var(--warning)]"
                role="alert"
                data-testid="assignment-missing-default"
              >
                {t(locale, 'assistant.subagentAssignment.missingDefaultBinding')}
              </div>
            )
          ) : null}

          {(mode === 'random' || mode === 'custom') && !hasValidKeys ? (
            <div className="rounded border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-xs text-[var(--warning)]">
              {t(locale, 'assistant.subagentAssignment.needValidKey')}
            </div>
          ) : null}

          {mode === 'random' && hasValidKeys ? (
            <div className="text-xs text-[var(--text-secondary)]">
              {t(locale, 'assistant.subagentAssignment.randomPoolHint').replace(
                '{count}',
                String(validKeys.length),
              )}
            </div>
          ) : null}

          {mode === 'custom' && hasValidKeys ? (
            <div>
              <div className="mb-1 text-[11px] font-medium text-[var(--text-disabled)]">
                {t(locale, 'assistant.subagentAssignment.bindings')}
              </div>
              <div className="space-y-2">
                {tasks.map((task, index) => {
                  const b = bindings[index] ?? {
                    providerId: validKeys[0]?.providerId ?? '',
                    keyId: validKeys[0]?.keyId ?? '',
                    modelId:
                      validKeys[0]?.modelId || validKeys[0]?.models[0]?.id || '',
                  };
                  const keyOpts = validKeys.filter((k) => k.providerId === b.providerId);
                  const providerIds = Array.from(new Set(validKeys.map((k) => k.providerId)));
                  const models =
                    validKeys.find(
                      (k) => k.providerId === b.providerId && k.keyId === b.keyId,
                    )?.models ?? [];
                  return (
                    <div
                      key={task.callId}
                      className="space-y-1 rounded border border-[var(--border)] p-2"
                      data-testid={`assignment-row-${task.callId}`}
                    >
                      <div className="truncate text-[11px] font-medium text-[var(--text)]">
                        {task.name}
                      </div>
                      <div className="grid grid-cols-3 gap-1.5">
                        <select
                          className="rounded border border-[var(--border)] bg-[var(--background)] px-2 py-1 text-xs"
                          value={b.providerId}
                          disabled={submitting}
                          onChange={(e) => {
                            const pid = e.target.value;
                            const first = validKeys.find((k) => k.providerId === pid);
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
                              validKeys.find((k) => k.providerId === pid)?.providerName ?? pid;
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
                          {(keyOpts.length ? keyOpts : validKeys).map((k) => (
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
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}

          {error ? (
            <div
              role="alert"
              className="rounded border border-red-400/30 bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-950/20"
            >
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
            disabled={confirmDisabled}
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
