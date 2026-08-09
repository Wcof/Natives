'use client';

import { useState } from 'react';
import { Pencil, Plug, Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { deleteCapabilityMcpServer } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityMcpServer } from '@/types/capability';

interface ConnectorListProps {
  locale: Locale;
  gateway: AssistantGateway;
  servers: CapabilityMcpServer[];
  onEdit: (server: CapabilityMcpServer) => void;
  onChanged: () => void;
}

function statusBadge(locale: Locale, status: string | null) {
  if (!status) return null;
  const normalized = status.toLowerCase();
  const running = normalized === 'running' || normalized === 'connected' || normalized === 'ready';
  const failed = normalized === 'error' || normalized === 'failed' || normalized === 'crashed';
  const label = running
    ? t(locale, 'capabilities.connectors.statusRunning')
    : failed
      ? t(locale, 'capabilities.connectors.statusError')
      : t(locale, 'capabilities.connectors.statusStopped');
  const color = running ? 'var(--success)' : failed ? 'var(--danger)' : 'var(--text-disabled)';
  return (
    <span className="flex items-center gap-1 text-[10px]" style={{ color }}>
      <span className="h-1.5 w-1.5 rounded-full" style={{ background: color }} aria-hidden />
      {label}
    </span>
  );
}

/** Connector rows with runtime status badge, trust/enable badges and delete confirm. */
export default function ConnectorList({ locale, gateway, servers, onEdit, onChanged }: ConnectorListProps) {
  const { toast } = useToast();
  const [confirmDelete, setConfirmDelete] = useState<CapabilityMcpServer | null>(null);

  const handleDelete = async () => {
    if (!confirmDelete) return;
    try {
      await deleteCapabilityMcpServer(gateway, confirmDelete.id);
      toast(t(locale, 'capabilities.connectors.deleted'), 'success');
      onChanged();
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    } finally {
      setConfirmDelete(null);
    }
  };

  return (
    <div className="flex flex-col gap-1">
      {servers.map((server) => (
        <div
          key={server.id}
          className="flex items-center gap-2.5 rounded-lg border px-3 py-2"
          style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
        >
          <Plug size={16} className="shrink-0" style={{ color: 'var(--primary)' }} aria-hidden />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="truncate text-sm font-medium" style={{ color: 'var(--text)' }}>
                {server.name}
              </span>
              <span
                className="rounded px-1.5 py-0.5 font-mono text-[10px] uppercase"
                style={{ background: 'var(--surface-hover)', color: 'var(--text-disabled)' }}
              >
                {server.transport}
              </span>
              <span
                className="rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
                style={{
                  borderColor: 'var(--border-subtle)',
                  color: server.enabled ? 'var(--success)' : 'var(--text-disabled)',
                }}
              >
                {server.enabled
                  ? t(locale, 'capabilities.common.enabled')
                  : t(locale, 'capabilities.common.disabled')}
              </span>
              <span
                className="rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
                style={{
                  borderColor: 'var(--border-subtle)',
                  color: server.trusted ? 'var(--success)' : 'var(--warning)',
                }}
              >
                {server.trusted
                  ? t(locale, 'capabilities.common.trusted')
                  : t(locale, 'capabilities.common.untrusted')}
              </span>
              {statusBadge(locale, server.runtimeStatus)}
            </div>
            <div className="mt-0.5 truncate font-mono text-xs" style={{ color: 'var(--text-secondary)' }}>
              {server.transport === 'stdio'
                ? [server.command, ...(server.args ?? [])].filter(Boolean).join(' ')
                : server.url ?? ''}
            </div>
          </div>
          <button
            type="button"
            onClick={() => onEdit(server)}
            aria-label={t(locale, 'capabilities.common.edit')}
            title={t(locale, 'capabilities.common.edit')}
            className="rounded p-1.5 hover:bg-[var(--surface-hover)]"
            style={{ color: 'var(--text-secondary)' }}
          >
            <Pencil size={14} />
          </button>
          <button
            type="button"
            onClick={() => setConfirmDelete(server)}
            aria-label={t(locale, 'capabilities.common.delete')}
            title={t(locale, 'capabilities.common.delete')}
            className="rounded p-1.5 hover:bg-[var(--surface-hover)]"
            style={{ color: 'var(--danger)' }}
          >
            <Trash2 size={14} />
          </button>
        </div>
      ))}

      <ConfirmDialog
        open={confirmDelete !== null}
        title={t(locale, 'capabilities.connectors.deleteTitle')}
        message={t(locale, 'capabilities.connectors.deleteMessage')}
        confirmLabel={t(locale, 'capabilities.common.delete')}
        cancelLabel={t(locale, 'capabilities.common.cancel')}
        danger
        onConfirm={() => void handleDelete()}
        onCancel={() => setConfirmDelete(null)}
      />
    </div>
  );
}
