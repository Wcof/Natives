'use client';

import { WifiOff, RefreshCw, AlertTriangle } from 'lucide-react';
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
  const zh = locale.startsWith('zh');
  if (connection === 'connected' || connection === 'disconnected') return null;

  const messages: Record<string, [string, string]> = {
    starting_daemon: ['正在启动引擎…', 'Starting engine…'],
    connecting: ['正在连接…', 'Connecting…'],
    reconnecting: ['正在重连', 'Reconnecting'],
    recovering: ['正在恢复事件…', 'Recovering events…'],
    offline: ['已离线 — 内容与草稿已保留', 'Offline — content and drafts kept'],
    incompatible: ['协议不兼容', 'Protocol incompatible'],
    fatal: ['引擎致命错误', 'Engine fatal error'],
  };
  const pair = messages[connection] ?? [connection, connection];
  let text = zh ? pair[0] : pair[1];
  if (connection === 'reconnecting' && reconnectAttempts > 0) {
    text += ` (${reconnectAttempts})`;
  }
  if (connection === 'incompatible' && clientVersion && daemonVersion) {
    text += ` · client ${clientVersion} / daemon ${daemonVersion}`;
  }

  const tone =
    connection === 'fatal' || connection === 'incompatible'
      ? 'border-red-400/40 bg-red-50 text-red-700 dark:bg-red-950/30 dark:text-red-300'
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
      {onReconnect && (connection === 'offline' || connection === 'reconnecting') && (
        <button type="button" onClick={onReconnect} className="pointer-events-auto underline">
          {zh ? '重试' : 'Retry'}
        </button>
      )}
      {onRestartDaemon && (connection === 'fatal' || connection === 'offline') && (
        <button type="button" onClick={onRestartDaemon} className="pointer-events-auto underline">
          {zh ? '重启引擎' : 'Restart engine'}
        </button>
      )}
      {onCopyDiagnostics && (
        <button type="button" onClick={onCopyDiagnostics} className="pointer-events-auto underline">
          {zh ? '复制诊断' : 'Copy diagnostics'}
        </button>
      )}
    </div>
  );
}
