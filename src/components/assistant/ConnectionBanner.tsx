'use client';

import { WifiOff, RefreshCw, AlertTriangle } from 'lucide-react';
import { t } from '@/i18n';
import type { ConnectionState } from '@/lib/assistant-protocol';

interface ConnectionBannerProps {
  connection: ConnectionState;
  error?: string | null;
  reconnectAttempts?: number;
  locale: string;
  onReconnect?: () => void;
  onRestartDaemon?: () => void;
  onCopyDiagnostics?: () => void;
  clientVersion?: string;
  daemonVersion?: string;
}

export default function ConnectionBanner({
  connection,
  error,
  reconnectAttempts = 0,
  locale,
  onReconnect,
  onRestartDaemon,
  onCopyDiagnostics,
  clientVersion,
  daemonVersion,
}: ConnectionBannerProps) {
  if (connection === 'connected' || connection === 'disconnected') return null;

  const messageKeys: Record<string, string> = {
    starting_daemon: 'connectionBanner.startingDaemon',
    connecting: 'connectionBanner.connecting',
    reconnecting: 'connectionBanner.reconnecting',
    recovering: 'connectionBanner.recovering',
    offline: 'connectionBanner.offline',
    incompatible: 'connectionBanner.incompatible',
    fatal: 'connectionBanner.fatal',
  };
  const messageKey = messageKeys[connection];
  let text = messageKey ? t(locale, messageKey) : connection;
  if (connection === 'reconnecting' && reconnectAttempts > 0) {
    text += ` (${reconnectAttempts})`;
  }
  if (connection === 'incompatible' && clientVersion && daemonVersion) {
    text += ` · client ${clientVersion} / daemon ${daemonVersion}`;
  }
  if (connection === 'incompatible') {
    // Retry can never fix a version mismatch — guide towards diagnostics/upgrade.
    text += t(locale, 'connectionBanner.incompatibleHint');
  }

  const tone =
    connection === 'fatal' || connection === 'incompatible'
      ? 'border-[var(--danger)]/40 bg-[var(--danger-soft)] text-[var(--danger)]'
      : connection === 'offline'
        ? 'border-[var(--border)] bg-[var(--surface-hover)] text-[var(--text-secondary)]'
        : 'border-[var(--primary)]/30 bg-[var(--primary)]/5 text-[var(--text-secondary)]';

  // Overlay: do not push the timeline/composer layout when reconnecting.
  return (
    <div
      className={`pointer-events-none absolute inset-x-0 top-0 z-30 flex items-center gap-2 border-b px-4 py-2 text-xs shadow-sm backdrop-blur-sm ${tone}`}
      role="status"
    >
      {connection === 'offline' || connection === 'fatal' ? (
        <WifiOff size={14} />
      ) : connection === 'incompatible' ? (
        <AlertTriangle size={14} />
      ) : (
        <RefreshCw size={14} className="animate-spin" />
      )}
      <span className="flex-1">
        {text}
        {error ? ` — ${error}` : ''}
      </span>
      {/* fatal is retryable via reconnect too — a dead engine must never be a dead end.
          incompatible is deliberately excluded: retry gives false hope there. */}
      {onReconnect && (connection === 'offline' || connection === 'reconnecting' || connection === 'fatal') && (
        <button type="button" onClick={onReconnect} className="pointer-events-auto underline">
          {t(locale, 'common.retry')}
        </button>
      )}
      {onRestartDaemon && (connection === 'fatal' || connection === 'offline') && (
        <button type="button" onClick={onRestartDaemon} className="pointer-events-auto underline">
          {t(locale, 'connectionBanner.restartEngine')}
        </button>
      )}
      {onCopyDiagnostics && (
        <button type="button" onClick={onCopyDiagnostics} className="pointer-events-auto underline">
          {t(locale, 'assistant.engineCopyDiagnostics')}
        </button>
      )}
    </div>
  );
}
