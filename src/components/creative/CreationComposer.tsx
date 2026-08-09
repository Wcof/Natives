'use client';

import { useState } from 'react';
import { t, type Locale } from '@/i18n';

interface CreationComposerProps {
  locale: Locale;
  /** Submit the idea. Resolves when the draft exists; rejects with a message to show. */
  onCreate: (intent: string, name?: string) => Promise<void>;
  /** Secondary path for people who already have an app to bring in. */
  onImport: () => void;
  disabled?: boolean;
}

/** Long enough to be a real request, short enough to stay one sentence. */
const MAX_INTENT_LENGTH = 500;

/**
 * The first screen of 个人创意: describe an idea, get an app.
 *
 * P0 deliberately asks for nothing else — no module id, no port, no
 * permissions, no tech stack (ADR-0014 section 2). Every field added here is a field
 * between the user and their idea; the host generates the id, and permissions
 * are confirmed at publish time when there is something real to judge.
 */
export default function CreationComposer({
  locale,
  onCreate,
  onImport,
  disabled = false,
}: CreationComposerProps) {
  const [intent, setIntent] = useState('');
  const [name, setName] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmed = intent.trim();
  const canSubmit = trimmed.length > 0 && !submitting && !disabled;

  const submit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    try {
      await onCreate(trimmed, name.trim() || undefined);
      setIntent('');
      setName('');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  // Cmd/Ctrl+Enter submits; plain Enter stays a newline so a multi-sentence
  // idea does not get cut off mid-thought.
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      void submit();
    }
  };

  return (
    <section className="rounded-lg border border-[var(--border)] bg-[var(--surface)] p-6">
      <h2 className="text-lg font-semibold text-[var(--text)]">
        {t(locale, 'creative.composerTitle')}
      </h2>
      <p className="mt-1 text-sm text-[var(--text-secondary)]">
        {t(locale, 'creative.composerSubtitle')}
      </p>

      <textarea
        id="creative-intent-input"
        value={intent}
        onChange={(e) => setIntent(e.target.value.slice(0, MAX_INTENT_LENGTH))}
        onKeyDown={onKeyDown}
        disabled={disabled || submitting}
        rows={3}
        placeholder={t(locale, 'creative.intentPlaceholder')}
        className="mt-4 w-full resize-none rounded-md border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--text)] outline-none placeholder:text-[var(--text-disabled)] focus:border-[var(--primary)] disabled:opacity-60"
      />

      <div className="mt-3 flex flex-wrap items-center gap-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          disabled={disabled || submitting}
          placeholder={t(locale, 'creative.namePlaceholder')}
          className="min-w-0 flex-1 rounded-md border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--text)] outline-none placeholder:text-[var(--text-disabled)] focus:border-[var(--primary)] disabled:opacity-60"
        />
        <button
          type="button"
          onClick={() => void submit()}
          disabled={!canSubmit}
          className="rounded-md bg-[var(--primary)] px-4 py-2 text-sm font-medium text-[var(--accent-ink)] disabled:opacity-40"
        >
          {submitting
            ? t(locale, 'creative.generating')
            : t(locale, 'creative.generateApp')}
        </button>
        <button
          type="button"
          onClick={onImport}
          disabled={disabled || submitting}
          className="rounded-md border border-[var(--border)] px-4 py-2 text-sm text-[var(--text-secondary)] disabled:opacity-40"
        >
          {t(locale, 'creative.importExisting')}
        </button>
      </div>

      {error && (
        <p className="mt-3 text-sm text-[var(--danger)]" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
