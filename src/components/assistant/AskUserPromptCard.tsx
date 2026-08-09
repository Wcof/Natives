'use client';

/**
 * Model → user question prompt (ask_user interaction).
 * Built on InteractionPromptShell so it can replace the composer like permissions.
 */

import { useCallback, useState } from 'react';
import { MessageCircleQuestion } from 'lucide-react';
import { t } from '@/i18n';
import { InteractionPromptShell } from './InteractionPromptShell';
import type { AskUserInteraction } from '@/lib/assistant-protocol';

export interface AskUserPromptCardProps {
  interaction: AskUserInteraction;
  locale: string;
  onAnswer: (id: string, answer: string) => void | Promise<void>;
  onCancel?: (id: string) => void | Promise<void>;
}

function errorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error && err.message.trim()) return err.message;
  if (typeof err === 'string' && err.trim()) return err;
  return fallback;
}

export default function AskUserPromptCard({
  interaction,
  locale,
  onAnswer,
  onCancel,
}: AskUserPromptCardProps) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [freeText, setFreeText] = useState('');

  const title = t(locale, 'assistant.askUser.title');
  const processing = t(locale, 'assistant.permission.processing');
  const errorFallback = t(locale, 'assistant.permission.errorFallback');
  const cancelLabel = t(locale, 'assistant.permission.reject');
  const submitLabel = t(locale, 'askUserCard.submit');

  const run = useCallback(
    async (action: () => void | Promise<void>) => {
      if (submitting) return;
      setSubmitting(true);
      setError(null);
      try {
        await action();
      } catch (err) {
        setError(errorMessage(err, errorFallback));
        setSubmitting(false);
      }
    },
    [submitting, errorFallback],
  );

  const options = interaction.question.options ?? [];
  const freeTextEnabled = Boolean(interaction.question.freeText) || options.length === 0;

  const focusRing =
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] focus-visible:ring-offset-1 focus-visible:ring-offset-[var(--surface)]';

  return (
    <InteractionPromptShell
      title={title}
      icon={<MessageCircleQuestion size={14} className="text-[var(--primary)]" />}
      tone="accent"
      submitting={submitting}
      processingLabel={processing}
      error={error}
      onEscape={
        onCancel
          ? () => {
              void run(() => onCancel(interaction.id));
            }
          : undefined
      }
      data-testid="ask-user-prompt-card"
    >
      <div className="space-y-3" data-ask-user-card>
        <p className="text-sm text-[var(--text)] break-words">{interaction.question.prompt}</p>

        {options.length > 0 ? (
          <div className="flex w-full flex-col gap-2" role="group">
            {options.map((opt) => (
              <button
                key={opt.id}
                type="button"
                disabled={submitting}
                onClick={() => void run(() => onAnswer(interaction.id, opt.id))}
                className={`w-full rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-left text-xs font-medium text-[var(--text)] transition-colors hover:bg-[var(--surface-hover)] disabled:cursor-not-allowed disabled:opacity-60 ${focusRing}`}
              >
                <span className="block">{opt.label}</span>
                {opt.description ? (
                  <span className="mt-0.5 block text-[11px] font-normal text-[var(--text-secondary)]">
                    {opt.description}
                  </span>
                ) : null}
              </button>
            ))}
          </div>
        ) : null}

        {freeTextEnabled ? (
          <div className="space-y-2">
            <textarea
              value={freeText}
              onChange={(e) => setFreeText(e.target.value)}
              disabled={submitting}
              rows={3}
              placeholder={t(locale, 'askUserCard.inputPlaceholder')}
              className="w-full resize-none rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--text)] placeholder:text-[var(--text-disabled)] disabled:opacity-60"
            />
            <button
              type="button"
              disabled={submitting || !freeText.trim()}
              onClick={() => void run(() => onAnswer(interaction.id, freeText.trim()))}
              className={`w-full rounded-lg bg-[var(--primary)] px-3 py-2 text-xs font-medium text-white disabled:cursor-not-allowed disabled:opacity-50 ${focusRing}`}
            >
              {submitLabel}
            </button>
          </div>
        ) : null}

        {onCancel ? (
          <button
            type="button"
            disabled={submitting}
            onClick={() => void run(() => onCancel(interaction.id))}
            className={`w-full rounded-lg border border-red-300/50 bg-transparent px-3 py-2 text-left text-xs font-medium text-red-600 dark:border-red-800/50 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/30 disabled:opacity-60 ${focusRing}`}
          >
            {cancelLabel}
          </button>
        ) : null}
      </div>
    </InteractionPromptShell>
  );
}
