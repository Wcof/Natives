'use client';

import { useCallback, useId, useState, type KeyboardEvent } from 'react';
import { Shield, ChevronDown, ChevronRight, Loader2 } from 'lucide-react';
import { t } from '@/i18n';

/** Scope strings accepted by daemon normalize_permission_scope — do not rename. */
export type PermissionScope = 'once' | 'this_run' | 'project';

/** Approve scopes in display order (recommended → broader). */
export const PERMISSION_APPROVE_SCOPES: readonly PermissionScope[] = [
  'once',
  'this_run',
  'project',
] as const;

export interface PermissionRequest {
  id: string;
  toolName: string;
  reason: string;
  input: Record<string, unknown>;
  status: 'pending' | 'approved' | 'rejected' | 'expired';
  createdAt: string;
}

export interface PermissionRequestCardProps {
  request: PermissionRequest;
  /**
   * May return a Promise. Card awaits it; on failure unlocks buttons and shows error.
   * Scope must stay one of: once | this_run | project.
   */
  onApprove: (id: string, scope: PermissionScope) => void | Promise<void>;
  /** May return a Promise. Card awaits it; on failure unlocks and shows error. */
  onReject: (id: string) => void | Promise<void>;
  locale: string;
}

export type PermissionCopy = {
  title: string;
  showDetails: string;
  hideDetails: string;
  allowOnce: string;
  allowThisRun: string;
  allowProject: string;
  reject: string;
  processing: string;
  errorFallback: string;
  /** Documented Escape behavior for a11y / tests. */
  escapeHint: string;
};

export function permissionCopy(locale: string): PermissionCopy {
  return {
    title: t(locale, 'assistant.permission.title'),
    showDetails: t(locale, 'assistant.permission.showDetails'),
    hideDetails: t(locale, 'assistant.permission.hideDetails'),
    allowOnce: t(locale, 'assistant.permission.allowOnce'),
    allowThisRun: t(locale, 'assistant.permission.allowThisRun'),
    allowProject: t(locale, 'assistant.permission.allowProject'),
    reject: t(locale, 'assistant.permission.reject'),
    processing: t(locale, 'assistant.permission.processing'),
    errorFallback: t(locale, 'assistant.permission.errorFallback'),
    escapeHint: t(locale, 'assistant.permission.escapeHint'),
  };
}

export function labelForScope(scope: PermissionScope, copy: PermissionCopy): string {
  switch (scope) {
    case 'once':
      return copy.allowOnce;
    case 'this_run':
      return copy.allowThisRun;
    case 'project':
      return copy.allowProject;
    default: {
      const _exhaustive: never = scope;
      return _exhaustive;
    }
  }
}

function errorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error && err.message.trim()) return err.message;
  if (typeof err === 'string' && err.trim()) return err;
  return fallback;
}

/**
 * Full-width permission card aligned with the composer column (max 860px).
 * Vertical action stack; async approve/reject with in-card error recovery.
 *
 * Keyboard:
 * - Tab / Shift+Tab traverse action buttons
 * - Enter activates the focused button (native button behavior)
 * - Escape rejects the request when not submitting
 * - focus-visible ring on interactive controls
 */
export default function PermissionRequestCard({
  request,
  onApprove,
  onReject,
  locale,
}: PermissionRequestCardProps) {
  const copy = permissionCopy(locale);
  const detailsId = useId();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runAction = useCallback(
    async (action: () => void | Promise<void>) => {
      if (submitting) return;
      setSubmitting(true);
      setError(null);
      try {
        await action();
        // Keep locked on success; parent clears pending request / unmounts card.
      } catch (err) {
        setError(errorMessage(err, copy.errorFallback));
        setSubmitting(false);
      }
    },
    [submitting, copy.errorFallback],
  );

  const handleApprove = useCallback(
    (scope: PermissionScope) => {
      void runAction(() => onApprove(request.id, scope));
    },
    [onApprove, request.id, runAction],
  );

  const handleReject = useCallback(() => {
    void runAction(() => onReject(request.id));
  }, [onReject, request.id, runAction]);

  const onCardKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== 'Escape') return;
      if (submitting) return;
      event.preventDefault();
      event.stopPropagation();
      handleReject();
    },
    [handleReject, submitting],
  );

  if (request.status !== 'pending') {
    return null;
  }

  const hasInput =
    request.input != null &&
    typeof request.input === 'object' &&
    Object.keys(request.input).length > 0;

  const focusRing =
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-1 focus-visible:ring-offset-[var(--surface)]';

  return (
    <div
      role="region"
      aria-label={copy.title}
      aria-busy={submitting}
      data-permission-card
      data-submitting={submitting ? 'true' : 'false'}
      tabIndex={-1}
      onKeyDown={onCardKeyDown}
      className="w-full my-3 rounded-xl border border-yellow-400/30 bg-yellow-50/50 dark:bg-yellow-950/10 overflow-hidden"
    >
      {/* Header */}
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-yellow-400/20 bg-yellow-50/80 dark:bg-yellow-950/20">
        <Shield size={14} className="text-yellow-600 dark:text-yellow-400 shrink-0" aria-hidden />
        <span className="text-xs font-semibold text-yellow-700 dark:text-yellow-300">
          {copy.title}
        </span>
        {submitting && (
          <span
            className="ml-auto inline-flex items-center gap-1.5 text-[11px] text-yellow-700/80 dark:text-yellow-300/80"
            data-permission-processing
          >
            <Loader2 size={12} className="animate-spin" aria-hidden />
            {copy.processing}
          </span>
        )}
      </div>

      {/* Body */}
      <div className="px-4 py-3 space-y-3">
        <div className="space-y-1">
          <div
            className="text-sm font-mono font-medium text-[var(--text-primary)] break-all"
            data-permission-tool
          >
            {request.toolName}
          </div>
          {request.reason ? (
            <p className="text-xs text-[var(--text-secondary)] break-words" data-permission-reason>
              {request.reason}
            </p>
          ) : null}
        </div>

        {/* Collapsible raw input — not shown expanded by default */}
        {hasInput ? (
          <div>
            <button
              type="button"
              aria-expanded={detailsOpen}
              aria-controls={detailsId}
              disabled={submitting}
              onClick={() => setDetailsOpen((open) => !open)}
              className={`inline-flex items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-50 ${focusRing} rounded`}
              data-permission-details-toggle
            >
              {detailsOpen ? (
                <ChevronDown size={12} aria-hidden />
              ) : (
                <ChevronRight size={12} aria-hidden />
              )}
              {detailsOpen ? copy.hideDetails : copy.showDetails}
            </button>
            {detailsOpen ? (
              <div
                id={detailsId}
                className="mt-2 bg-[var(--surface)] rounded-lg p-2 text-xs font-mono text-[var(--text-secondary)] overflow-x-auto max-w-full"
                data-permission-details
              >
                <pre className="whitespace-pre-wrap break-all m-0">
                  {JSON.stringify(request.input, null, 2)}
                </pre>
              </div>
            ) : null}
          </div>
        ) : null}

        {/* Vertical full-width actions: approve scopes then reject */}
        <div className="flex flex-col gap-2 w-full" data-permission-actions role="group" aria-label={copy.title}>
          {PERMISSION_APPROVE_SCOPES.map((scope) => (
            <button
              key={scope}
              type="button"
              data-permission-scope={scope}
              disabled={submitting}
              onClick={() => handleApprove(scope)}
              className={`w-full px-3 py-2 rounded-lg text-xs font-medium text-left transition-colors disabled:opacity-60 disabled:cursor-not-allowed ${focusRing} bg-green-100 dark:bg-green-950/30 text-green-700 dark:text-green-400 hover:bg-green-200 dark:hover:bg-green-950/50`}
            >
              {labelForScope(scope, copy)}
            </button>
          ))}

          <button
            type="button"
            data-permission-reject
            disabled={submitting}
            onClick={handleReject}
            className={`w-full px-3 py-2 rounded-lg text-xs font-medium text-left transition-colors disabled:opacity-60 disabled:cursor-not-allowed ${focusRing} bg-transparent border border-red-300/50 dark:border-red-800/50 text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/30`}
          >
            {copy.reject}
          </button>
        </div>

        {error ? (
          <div
            role="alert"
            data-permission-error
            className="text-xs text-red-600 dark:text-red-400 break-words"
          >
            {error}
          </div>
        ) : null}

        <p className="sr-only">{copy.escapeHint}</p>
      </div>
    </div>
  );
}
