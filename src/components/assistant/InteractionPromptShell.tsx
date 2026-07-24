'use client';

/**
 * Shared shell for assistant interactions that require an explicit user choice
 * (tool permission, model ask-user, plan approval, …).
 *
 * Layout contract:
 * - Sits in the composer column (`mx-auto w-full max-w-[860px] px-5`).
 * - Covers the entire MessageInput area so the user cannot type / stop / send
 *   while a choice is pending (avoids racing the run with new input).
 * - Body is free-form; callers inject their own action buttons.
 */

import { useCallback, type KeyboardEvent, type ReactNode } from 'react';
import { Loader2 } from 'lucide-react';

export const COMPOSER_COLUMN_CLASS = 'mx-auto w-full max-w-[860px] px-5';

export interface InteractionPromptShellProps {
  /** Accessible name for the region. */
  title: string;
  /** Optional icon left of the title (e.g. Shield). */
  icon?: ReactNode;
  /** Optional header trailing content (status chip, etc.). */
  headerExtra?: ReactNode;
  /** When true, show a processing indicator in the header. */
  submitting?: boolean;
  processingLabel?: string;
  /** In-card error after a failed async action. */
  error?: string | null;
  /** Escape key handler (typically reject / cancel). */
  onEscape?: () => void;
  /** Optional tone for border/background. */
  tone?: 'warning' | 'neutral' | 'accent';
  children: ReactNode;
  /** Optional footer (action stack). Prefer placing actions inside children. */
  footer?: ReactNode;
  /** Test / analytics hook. */
  'data-testid'?: string;
}

const TONE_CLASS: Record<NonNullable<InteractionPromptShellProps['tone']>, string> = {
  warning:
    'border-[var(--warning)]/30 bg-[var(--surface)] shadow-[var(--shadow-modal)]',
  neutral: 'border-[var(--border)] bg-[var(--surface)] shadow-[var(--shadow-modal)]',
  accent: 'border-[var(--primary)]/30 bg-[var(--surface)] shadow-[var(--shadow-modal)]',
};

const TONE_HEADER: Record<NonNullable<InteractionPromptShellProps['tone']>, string> = {
  warning:
    'border-b border-[var(--border-subtle)] bg-[var(--warning-soft)] text-[var(--text)]',
  neutral:
    'border-b border-[var(--border-subtle)] bg-[var(--surface-hover)]/60 text-[var(--text-secondary)]',
  accent:
    'border-b border-[var(--border-subtle)] bg-[var(--primary-soft)] text-[var(--text)]',
};

/**
 * Full-width interaction card used as an overlay over the composer.
 * Does not include outer column padding — wrap with {@link COMPOSER_COLUMN_CLASS}
 * or use {@link InteractionPromptOverlay}.
 */
export function InteractionPromptShell({
  title,
  icon,
  headerExtra,
  submitting = false,
  processingLabel,
  error = null,
  onEscape,
  tone = 'warning',
  children,
  footer,
  'data-testid': testId,
}: InteractionPromptShellProps) {
  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== 'Escape') return;
      if (submitting) return;
      if (!onEscape) return;
      event.preventDefault();
      event.stopPropagation();
      onEscape();
    },
    [onEscape, submitting],
  );

  return (
    <div
      role="region"
      aria-label={title}
      aria-busy={submitting}
      data-interaction-prompt
      data-testid={testId}
      data-submitting={submitting ? 'true' : 'false'}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className={`w-full overflow-hidden rounded-[var(--radius-lg,14px)] border ${TONE_CLASS[tone]}`}
    >
      <div
        className={`flex items-center gap-2 px-4 py-2.5 text-xs font-medium ${TONE_HEADER[tone]}`}
      >
        {icon ? <span className="shrink-0" aria-hidden>{icon}</span> : null}
        <span className="min-w-0 flex-1 truncate font-semibold">{title}</span>
        {headerExtra}
        {submitting && processingLabel ? (
          <span
            className="ml-auto inline-flex items-center gap-1.5 text-[11px] opacity-80"
            data-interaction-processing
          >
            <Loader2 size={12} className="animate-spin" aria-hidden />
            {processingLabel}
          </span>
        ) : null}
      </div>

      <div className="space-y-3 px-4 py-3">{children}</div>

      {error ? (
        <div
          role="alert"
          className="mx-4 mb-3 rounded-lg border border-[var(--danger)]/30 bg-[var(--danger-soft)] px-3 py-2 text-xs text-[var(--danger)]"
          data-interaction-error
        >
          {error}
        </div>
      ) : null}

      {footer ? <div className="border-t border-[var(--border-subtle)] px-4 py-3">{footer}</div> : null}
    </div>
  );
}

export interface InteractionPromptOverlayProps extends InteractionPromptShellProps {
  /**
   * When true, the shell is shown in the composer column and is meant to replace
   * MessageInput for the duration of the interaction.
   */
  active: boolean;
}

/**
 * Composer-column wrapper: same horizontal geometry as MessageInput.
 * Parent should hide MessageInput while `active` so the user cannot type/stop.
 */
export function InteractionPromptOverlay({
  active,
  ...shellProps
}: InteractionPromptOverlayProps) {
  if (!active) return null;
  return (
    <div className={`${COMPOSER_COLUMN_CLASS} pb-5 pt-2`} data-interaction-overlay>
      <InteractionPromptShell {...shellProps} />
    </div>
  );
}

export default InteractionPromptShell;
