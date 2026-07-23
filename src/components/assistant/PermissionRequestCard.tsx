'use client';

import { useCallback, useId, useState } from 'react';
import { Shield, ChevronDown, ChevronRight } from 'lucide-react';
import { t } from '@/i18n';
import {
  InteractionPromptShell,
} from './InteractionPromptShell';

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
 * Tool permission choice UI built on {@link InteractionPromptShell}.
 * Parent should mount this as a composer overlay (hide MessageInput while open).
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
    <InteractionPromptShell
      title={copy.title}
      icon={<Shield size={14} className="text-yellow-600 dark:text-yellow-400" />}
      tone="warning"
      submitting={submitting}
      processingLabel={copy.processing}
      error={error}
      onEscape={handleReject}
      data-testid="permission-request-card"
    >
      <div className="space-y-3" data-permission-card data-submitting={submitting ? 'true' : 'false'}>
        <div className="space-y-1">
          <div
            className="break-all text-sm font-mono font-medium text-[var(--text-primary)]"
            data-permission-tool
          >
            {request.toolName}
          </div>
          {request.reason ? (
            <p className="break-words text-xs text-[var(--text-secondary)]" data-permission-reason>
              {request.reason}
            </p>
          ) : null}
        </div>

        {hasInput ? (
          <div>
            <button
              type="button"
              aria-expanded={detailsOpen}
              aria-controls={detailsId}
              disabled={submitting}
              onClick={() => setDetailsOpen((open) => !open)}
              className={`inline-flex items-center gap-1 rounded text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-50 ${focusRing}`}
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
                className="mt-2 max-w-full overflow-x-auto rounded-lg bg-[var(--surface)] p-2 text-xs font-mono text-[var(--text-secondary)]"
                data-permission-details
              >
                <pre className="m-0 whitespace-pre-wrap break-all">
                  {JSON.stringify(request.input, null, 2)}
                </pre>
              </div>
            ) : null}
          </div>
        ) : null}

        <div
          className="flex w-full flex-col gap-2"
          data-permission-actions
          role="group"
          aria-label={copy.title}
        >
          {PERMISSION_APPROVE_SCOPES.map((scope) => (
            <button
              key={scope}
              type="button"
              data-permission-scope={scope}
              disabled={submitting}
              onClick={() => handleApprove(scope)}
              className={`w-full rounded-lg px-3 py-2 text-left text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${focusRing} bg-green-100 text-green-700 hover:bg-green-200 dark:bg-green-950/30 dark:text-green-400 dark:hover:bg-green-950/50`}
            >
              {labelForScope(scope, copy)}
            </button>
          ))}

          <button
            type="button"
            data-permission-reject
            disabled={submitting}
            onClick={handleReject}
            className={`w-full rounded-lg border border-red-300/50 bg-transparent px-3 py-2 text-left text-xs font-medium text-red-600 transition-colors disabled:cursor-not-allowed disabled:opacity-60 dark:border-red-800/50 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/30 ${focusRing}`}
          >
            {copy.reject}
          </button>
        </div>

        <p className="sr-only">{copy.escapeHint}</p>
      </div>
    </InteractionPromptShell>
  );
}
