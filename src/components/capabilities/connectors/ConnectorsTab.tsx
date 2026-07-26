'use client';

import { useCallback, useEffect, useState } from 'react';
import { FileJson, Plus } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import { canBrowseMcpHub } from '@/lib/assistant-workspace/capability-gate';
import { listCapabilityMcpServers } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import type { CapabilityMcpServer } from '../shared/capability-types';
import ConnectorList from './ConnectorList';
import ConnectorEditDialog from './ConnectorEditDialog';
import ConnectorJsonImportDialog from './ConnectorJsonImportDialog';
import McpHubBrowser from './McpHubBrowser';

interface ConnectorsTabProps {
  locale: Locale;
  gateway: AssistantGateway;
  caps: DaemonCapabilities | null;
}

type Segment = 'mine' | 'hub';

export default function ConnectorsTab({ locale, gateway, caps }: ConnectorsTabProps) {
  const hubAvailable = canBrowseMcpHub(caps);
  const [segment, setSegment] = useState<Segment>('mine');
  const [servers, setServers] = useState<CapabilityMcpServer[]>([]);
  const [phase, setPhase] = useState<'loading' | 'error' | 'ready'>('loading');
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<CapabilityMcpServer | null>(null);
  const [creating, setCreating] = useState(false);
  const [jsonImportOpen, setJsonImportOpen] = useState(false);

  const load = useCallback(async () => {
    setPhase('loading');
    setError(null);
    try {
      const list = await listCapabilityMcpServers(gateway);
      setServers(list);
      setPhase('ready');
    } catch (e) {
      setError(classifyError(e).userMessage);
      setPhase('error');
    }
  }, [gateway]);

  useEffect(() => {
    void load();
  }, [load]);

  const segments: { id: Segment; label: string }[] = [
    { id: 'mine', label: t(locale, 'capabilities.connectors.mine') },
    // Honest gate: hub segment only when capability.mcp.hub.search is advertised.
    ...(hubAvailable ? [{ id: 'hub' as Segment, label: t(locale, 'capabilities.connectors.browseHub') }] : []),
  ];

  return (
    <div className="flex h-full flex-col gap-3 overflow-hidden">
      <div className="flex flex-wrap items-center gap-2">
        <div
          className="flex rounded-lg border p-0.5"
          style={{ borderColor: 'var(--border-subtle)' }}
          role="tablist"
          aria-label={t(locale, 'capabilities.tabs.connectors')}
        >
          {segments.map((s) => (
            <button
              key={s.id}
              type="button"
              role="tab"
              aria-selected={segment === s.id}
              onClick={() => setSegment(s.id)}
              className="rounded-md px-3 py-1 text-sm transition"
              style={{
                background: segment === s.id ? 'var(--surface-hover)' : 'transparent',
                color: segment === s.id ? 'var(--text)' : 'var(--text-secondary)',
              }}
            >
              {s.label}
            </button>
          ))}
        </div>
        {!hubAvailable ? (
          <span className="text-xs" style={{ color: 'var(--text-disabled)' }}>
            {t(locale, 'capabilities.gate.hubUnavailable')}
          </span>
        ) : null}
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={() => setJsonImportOpen(true)}
            className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm"
            style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
          >
            <FileJson size={14} aria-hidden />
            {t(locale, 'capabilities.connectors.importJson')}
          </button>
          <button
            type="button"
            onClick={() => setCreating(true)}
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm"
            style={{ background: 'var(--primary)', color: '#fff' }}
          >
            <Plus size={14} aria-hidden />
            {t(locale, 'capabilities.connectors.add')}
          </button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {segment === 'hub' && hubAvailable ? (
          <McpHubBrowser
            locale={locale}
            gateway={gateway}
            onInstalled={() => void load()}
            onFallbackToJson={() => {
              setSegment('mine');
              setJsonImportOpen(true);
            }}
          />
        ) : phase === 'loading' ? (
          <LoadingState message={t(locale, 'capabilities.common.loading')} />
        ) : phase === 'error' ? (
          <ErrorState
            message={error ?? t(locale, 'capabilities.connectors.loadFailed')}
            onRetry={() => void load()}
          />
        ) : servers.length === 0 ? (
          <EmptyState
            title={t(locale, 'capabilities.connectors.empty')}
            description={t(locale, 'capabilities.connectors.emptyDesc')}
            action={{ label: t(locale, 'capabilities.connectors.add'), onClick: () => setCreating(true) }}
          />
        ) : (
          <ConnectorList
            locale={locale}
            gateway={gateway}
            servers={servers}
            onEdit={setEditing}
            onChanged={() => void load()}
          />
        )}
      </div>

      {(creating || editing) && (
        <ConnectorEditDialog
          locale={locale}
          gateway={gateway}
          server={editing}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          onSaved={() => {
            setCreating(false);
            setEditing(null);
            void load();
          }}
        />
      )}
      <ConnectorJsonImportDialog
        locale={locale}
        gateway={gateway}
        open={jsonImportOpen}
        onClose={() => setJsonImportOpen(false)}
        onImported={() => void load()}
      />
    </div>
  );
}
