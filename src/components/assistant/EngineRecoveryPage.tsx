'use client';

import { AlertTriangle, Copy, RefreshCw, WifiOff } from 'lucide-react';
import type { ConnectionState } from '@/lib/assistant-protocol';
import { t, type Locale } from '@/i18n';

interface EngineRecoveryPageProps {
  locale: Locale | string;
  connection: ConnectionState;
  error?: string | null;
  onRetry?: () => void;
  onCopyDiagnostics?: () => void;
  clientVersion?: string;
  daemonVersion?: string;
}

export default function EngineRecoveryPage({
  locale,
  connection,
  error,
  onRetry,
  onCopyDiagnostics,
  clientVersion,
  daemonVersion,
}: EngineRecoveryPageProps) {
  const title = t(locale, 'assistant.engineRecoveryTitle');
  const body =
    connection === 'incompatible'
      ? t(locale, 'assistant.engineIncompatibleBody')
      : connection === 'fatal'
        ? t(locale, 'assistant.engineFatalBody')
        : t(locale, 'assistant.engineUnavailableBody');

  const versionLine =
    clientVersion || daemonVersion
      ? `client ${clientVersion ?? '—'} / daemon ${daemonVersion ?? '—'}`
      : null;

  return (
    <div
      className="flex h-full min-h-0 flex-1 flex-col items-center justify-center gap-4 px-6 text-center"
      data-testid="engine-recovery-page"
      role="alert"
    >
      <div className="flex h-14 w-14 items-center justify-center rounded-full border border-red-400/30 bg-red-50 text-red-600 dark:bg-red-950/30 dark:text-red-300">
        {connection === 'incompatible' ? <AlertTriangle size={24} /> : <WifiOff size={24} />}
      </div>
      <div className="max-w-md space-y-2">
        <h2 className="text-base font-semibold text-[var(--text)]">{title}</h2>
        <p className="text-sm text-[var(--text-secondary)]">{body}</p>
        {error ? (
          <p className="break-words font-mono text-xs text-[var(--danger)]">{error}</p>
        ) : null}
        {versionLine ? (
          <p className="text-[11px] text-[var(--text-disabled)]">{versionLine}</p>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center justify-center gap-2">
        {/* Retry can never fix a protocol/version mismatch — hide it for
            `incompatible` and promote copy-diagnostics to the primary action. */}
        {onRetry && connection !== 'incompatible' ? (
          <button
            type="button"
            onClick={onRetry}
            className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--primary)] px-3 py-1.5 text-sm text-white"
            data-testid="engine-recovery-retry"
          >
            <RefreshCw size={14} />
            {t(locale, 'assistant.engineRetryConnection')}
          </button>
        ) : null}
        {onCopyDiagnostics ? (
          <button
            type="button"
            onClick={onCopyDiagnostics}
            className={
              connection === 'incompatible'
                ? 'inline-flex items-center gap-1.5 rounded-lg bg-[var(--primary)] px-3 py-1.5 text-sm text-white'
                : 'inline-flex items-center gap-1.5 rounded-lg border border-[var(--border)] px-3 py-1.5 text-sm text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
            }
            data-testid="engine-recovery-copy"
          >
            <Copy size={14} />
            {t(locale, 'assistant.engineCopyDiagnostics')}
          </button>
        ) : null}
      </div>
    </div>
  );
}
