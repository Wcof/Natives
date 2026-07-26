'use client';

import { useState } from 'react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  createCapabilityMcpServer,
  updateCapabilityMcpServer,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityMcpServer, McpTransport } from '../shared/capability-types';
import KeyValueRows, { type KvRow } from './KeyValueRows';

interface ConnectorEditDialogProps {
  locale: Locale;
  gateway: AssistantGateway;
  /** null → create */
  server: CapabilityMcpServer | null;
  onClose: () => void;
  onSaved: () => void;
}

const inputStyle = {
  borderColor: 'var(--border)',
  background: 'var(--surface)',
  color: 'var(--text)',
} as const;

/**
 * Connector form. Editing an existing server shows env/header keys only (values
 * are never echoed back by the daemon); rows are sent only when touched, so an
 * untouched form never wipes stored values.
 */
export default function ConnectorEditDialog({ locale, gateway, server, onClose, onSaved }: ConnectorEditDialogProps) {
  const { toast } = useToast();
  const isEdit = server !== null;
  const [name, setName] = useState(server?.name ?? '');
  const [transport, setTransport] = useState<McpTransport>(server?.transport ?? 'stdio');
  const [command, setCommand] = useState(server?.command ?? '');
  const [argsText, setArgsText] = useState((server?.args ?? []).join('\n'));
  const [url, setUrl] = useState(server?.url ?? '');
  const [envRows, setEnvRows] = useState<KvRow[]>(
    (server?.env ?? []).map((e) => ({ key: e.key, value: '', isSecretRef: e.isSecretRef })),
  );
  const [headerRows, setHeaderRows] = useState<KvRow[]>(
    (server?.headerKeys ?? []).map((key) => ({ key, value: '', isSecretRef: false })),
  );
  const [envTouched, setEnvTouched] = useState(false);
  const [headersTouched, setHeadersTouched] = useState(false);
  const [authMode, setAuthMode] = useState(server?.authMode ?? '');
  const [trusted, setTrusted] = useState(server?.trusted ?? false);
  const [enabled, setEnabled] = useState(server?.enabled ?? false);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const validate = (): string | null => {
    if (!name.trim()) return t(locale, 'capabilities.connectors.nameRequired');
    if (transport === 'stdio' && !command.trim()) return t(locale, 'capabilities.connectors.commandRequired');
    if (transport !== 'stdio' && !url.trim()) return t(locale, 'capabilities.connectors.urlRequired');
    return null;
  };

  const collect = (rows: KvRow[]): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const row of rows) {
      if (row.key.trim()) out[row.key.trim()] = row.value;
    }
    return out;
  };

  const handleSave = async () => {
    const invalid = validate();
    if (invalid) {
      setFormError(invalid);
      return;
    }
    setFormError(null);
    setSubmitting(true);
    const payload = {
      name: name.trim(),
      transport,
      ...(transport === 'stdio'
        ? {
            command: command.trim(),
            args: argsText.split('\n').map((s) => s.trim()).filter(Boolean),
          }
        : { url: url.trim() }),
      ...(envTouched ? { env: collect(envRows) } : {}),
      ...(headersTouched ? { headers: collect(headerRows) } : {}),
      ...(authMode.trim() ? { authMode: authMode.trim() } : {}),
      trusted,
      enabled,
    };
    try {
      if (isEdit && server) {
        await updateCapabilityMcpServer(gateway, server.id, payload);
        toast(t(locale, 'capabilities.connectors.saved'), 'success');
      } else {
        await createCapabilityMcpServer(gateway, payload);
        toast(t(locale, 'capabilities.connectors.created'), 'success');
      }
      onSaved();
    } catch (e) {
      setFormError(classifyError(e).userMessage);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      isOpen
      onClose={onClose}
      title={t(locale, isEdit ? 'capabilities.connectors.editTitle' : 'capabilities.connectors.createTitle')}
      width={560}
    >
      <div className="space-y-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t(locale, 'capabilities.connectors.name')}
          aria-label={t(locale, 'capabilities.connectors.name')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
          autoFocus
        />

        <div className="flex items-center gap-2" role="radiogroup" aria-label={t(locale, 'capabilities.connectors.transport')}>
          <span className="text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.connectors.transport')}
          </span>
          {(['stdio', 'http', 'sse'] as const).map((option) => (
            <button
              key={option}
              type="button"
              role="radio"
              aria-checked={transport === option}
              onClick={() => setTransport(option)}
              className="rounded-lg border px-3 py-1 font-mono text-xs uppercase"
              style={{
                borderColor: transport === option ? 'var(--primary)' : 'var(--border-subtle)',
                color: transport === option ? 'var(--primary)' : 'var(--text-secondary)',
              }}
            >
              {option}
            </button>
          ))}
        </div>

        {transport === 'stdio' ? (
          <>
            <input
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              placeholder={t(locale, 'capabilities.connectors.command')}
              aria-label={t(locale, 'capabilities.connectors.command')}
              className="w-full rounded border px-3 py-2 font-mono text-sm"
              style={inputStyle}
            />
            <textarea
              value={argsText}
              onChange={(e) => setArgsText(e.target.value)}
              placeholder={t(locale, 'capabilities.connectors.argsPlaceholder')}
              aria-label={t(locale, 'capabilities.connectors.argsLabel')}
              className="h-16 w-full resize-none rounded border px-3 py-2 font-mono text-xs"
              style={inputStyle}
            />
            <div>
              <span className="mb-1 block text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'capabilities.connectors.env')}
              </span>
              <KeyValueRows
                locale={locale}
                rows={envRows}
                onRows={setEnvRows}
                onTouched={() => setEnvTouched(true)}
                withSecret
                keyLabel={t(locale, 'capabilities.connectors.envKey')}
                valueLabel={t(locale, 'capabilities.connectors.envValue')}
                addLabel={t(locale, 'capabilities.connectors.addEnv')}
              />
            </div>
          </>
        ) : (
          <>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder={t(locale, 'capabilities.connectors.url')}
              aria-label={t(locale, 'capabilities.connectors.url')}
              className="w-full rounded border px-3 py-2 font-mono text-sm"
              style={inputStyle}
            />
            <div>
              <span className="mb-1 block text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'capabilities.connectors.headers')}
              </span>
              <KeyValueRows
                locale={locale}
                rows={headerRows}
                onRows={setHeaderRows}
                onTouched={() => setHeadersTouched(true)}
                withSecret={false}
                keyLabel={t(locale, 'capabilities.connectors.headerKey')}
                valueLabel={t(locale, 'capabilities.connectors.headerValue')}
                addLabel={t(locale, 'capabilities.connectors.addHeader')}
              />
            </div>
          </>
        )}

        <input
          value={authMode}
          onChange={(e) => setAuthMode(e.target.value)}
          placeholder={t(locale, 'capabilities.connectors.authMode')}
          aria-label={t(locale, 'capabilities.connectors.authMode')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />

        <div className="rounded-lg border p-3" style={{ borderColor: 'var(--warning)' }}>
          <label className="flex items-center justify-between gap-2 text-sm" style={{ color: 'var(--text)' }}>
            {t(locale, 'capabilities.connectors.trustedLabel')}
            <input type="checkbox" checked={trusted} onChange={(e) => setTrusted(e.target.checked)} />
          </label>
          <p className="mt-1 text-xs" style={{ color: 'var(--warning)' }}>
            {t(locale, 'capabilities.connectors.trustedWarning')}
          </p>
        </div>
        <label className="flex items-center justify-between gap-2 text-sm" style={{ color: 'var(--text)' }}>
          {t(locale, 'capabilities.connectors.enabledLabel')}
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
        </label>

        {formError ? <p className="text-sm" style={{ color: 'var(--danger)' }}>{formError}</p> : null}

        <div className="flex justify-end gap-2 pt-1">
          <button type="button" onClick={onClose} className="px-4 py-2 text-sm" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={submitting}
            className="rounded px-4 py-2 text-sm disabled:opacity-50"
            style={{ background: 'var(--primary)', color: '#fff' }}
          >
            {t(locale, 'capabilities.common.save')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
