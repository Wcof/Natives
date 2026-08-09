'use client';

import { useEffect, useState } from 'react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  createCapabilityMcpServer,
  updateCapabilityMcpServer,
} from '@/lib/assistant-workspace/capability-admin';
import {
  createCapabilitySecret,
  hasCapabilitySecrets,
  listCapabilitySecrets,
  startMcpOauth,
  type CapabilitySecretEntry,
} from '@/lib/assistant-workspace/capability-secrets';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityMcpServer, McpAuthMode, McpTransport } from '@/types/capability';
import KeyValueRows, { collectKvRows, newKvRow, type KvRow } from './KeyValueRows';

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

const AUTH_MODES: McpAuthMode[] = ['none', 'bearer', 'oauth'];

const normalizeAuthMode = (raw: string | null | undefined): McpAuthMode =>
  raw === 'bearer' || raw === 'oauth' ? raw : 'none';

const oauthField = (config: Record<string, unknown> | null | undefined, key: string): string => {
  const v = config?.[key];
  return typeof v === 'string' ? v : '';
};

type OauthState = 'idle' | 'pending' | 'success' | 'error';

/**
 * Connector form. Editing an existing server shows env/header keys only (values
 * are never echoed by the daemon); untouched stored rows are submitted as `null`
 * — the daemon-side "keep stored value" sentinel — so editing one row never
 * wipes the others. See collectKvRows for the exact payload semantics.
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
    (server?.env ?? []).map((e) => newKvRow({ key: e.key, isSecretRef: e.isSecretRef, stored: true })),
  );
  const [headerRows, setHeaderRows] = useState<KvRow[]>(
    (server?.headerKeys ?? []).map((key) => newKvRow({ key, stored: true })),
  );
  const [envTouched, setEnvTouched] = useState(false);
  const [headersTouched, setHeadersTouched] = useState(false);
  const [authMode, setAuthMode] = useState<McpAuthMode>(normalizeAuthMode(server?.authMode));
  const [oauthAuthorizeUrl, setOauthAuthorizeUrl] = useState(oauthField(server?.oauthConfig, 'authorizeUrl'));
  const [oauthTokenUrl, setOauthTokenUrl] = useState(oauthField(server?.oauthConfig, 'tokenUrl'));
  const [oauthClientId, setOauthClientId] = useState(oauthField(server?.oauthConfig, 'clientId'));
  const [oauthScopes, setOauthScopes] = useState(() => {
    const scopes = server?.oauthConfig?.scopes;
    return Array.isArray(scopes) ? scopes.filter((s): s is string => typeof s === 'string').join(', ') : '';
  });
  const [oauthState, setOauthState] = useState<OauthState>('idle');
  const [oauthError, setOauthError] = useState<string | null>(null);
  const [secrets, setSecrets] = useState<CapabilitySecretEntry[]>([]);
  const [trusted, setTrusted] = useState(server?.trusted ?? false);
  const [enabled, setEnabled] = useState(server?.enabled ?? false);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const secretsAvailable = isEdit && hasCapabilitySecrets();
  const serverId = server?.id ?? null;

  useEffect(() => {
    if (!secretsAvailable || !serverId) return;
    let cancelled = false;
    listCapabilitySecrets(serverId)
      .then((entries) => {
        if (!cancelled) setSecrets(entries);
      })
      .catch(() => {
        if (!cancelled) toast(t(locale, 'capabilities.connectors.secretListFailed'), 'error');
      });
    return () => {
      cancelled = true;
    };
  }, [secretsAvailable, serverId, locale, toast]);

  const handleCreateSecret = async (keyName: string, plaintext: string): Promise<string> => {
    if (!serverId) throw new Error('secret creation requires a saved connector');
    const id = await createCapabilitySecret({ kind: 'mcp_env', ownerRef: serverId, keyName, plaintext });
    toast(t(locale, 'capabilities.connectors.secretCreated'), 'success');
    listCapabilitySecrets(serverId)
      .then(setSecrets)
      .catch(() => undefined);
    return id;
  };

  const oauthConfigReady =
    oauthAuthorizeUrl.trim().length > 0 && oauthTokenUrl.trim().length > 0 && oauthClientId.trim().length > 0;

  const handleAuthorize = async () => {
    if (!serverId || !oauthConfigReady || oauthState === 'pending') return;
    setOauthState('pending');
    setOauthError(null);
    try {
      const result = await startMcpOauth({
        serverId,
        authorizeUrl: oauthAuthorizeUrl.trim(),
        tokenUrl: oauthTokenUrl.trim(),
        clientId: oauthClientId.trim(),
        scopes: oauthScopes.split(',').map((s) => s.trim()).filter(Boolean),
      });
      setOauthState('success');
      toast(
        t(locale, result.hasRefresh ? 'capabilities.connectors.oauthSuccessRefresh' : 'capabilities.connectors.oauthSuccess'),
        'success',
      );
    } catch (e) {
      setOauthState('error');
      setOauthError(classifyError(e).userMessage);
    }
  };

  const validate = (): string | null => {
    if (!name.trim()) return t(locale, 'capabilities.connectors.nameRequired');
    if (transport === 'stdio' && !command.trim()) return t(locale, 'capabilities.connectors.commandRequired');
    if (transport !== 'stdio' && !url.trim()) return t(locale, 'capabilities.connectors.urlRequired');
    return null;
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
      ...(envTouched ? { env: collectKvRows(envRows) } : {}),
      ...(headersTouched ? { headers: collectKvRows(headerRows) } : {}),
      authMode,
      ...(authMode === 'oauth'
        ? {
            oauthConfig: {
              authorizeUrl: oauthAuthorizeUrl.trim(),
              tokenUrl: oauthTokenUrl.trim(),
              clientId: oauthClientId.trim(),
              scopes: oauthScopes.split(',').map((s) => s.trim()).filter(Boolean),
            },
          }
        : {}),
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
      closeOnEscape={!submitting}
      closeOnBackdropClick={!submitting}
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
                secrets={secrets}
                onCreateSecret={secretsAvailable ? handleCreateSecret : undefined}
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

        <div className="flex items-center gap-2">
          <label
            className="text-xs font-semibold uppercase"
            style={{ color: 'var(--text-secondary)' }}
            htmlFor="connector-auth-mode"
          >
            {t(locale, 'capabilities.connectors.authMode')}
          </label>
          <select
            id="connector-auth-mode"
            value={authMode}
            onChange={(e) => setAuthMode(normalizeAuthMode(e.target.value))}
            className="rounded border px-2 py-1.5 text-sm"
            style={inputStyle}
          >
            {AUTH_MODES.map((mode) => (
              <option key={mode} value={mode}>
                {t(locale, mode === 'none'
                  ? 'capabilities.connectors.authModeNone'
                  : mode === 'bearer'
                    ? 'capabilities.connectors.authModeBearer'
                    : 'capabilities.connectors.authModeOauth')}
              </option>
            ))}
          </select>
        </div>

        {authMode === 'oauth' ? (
          <div className="space-y-2 rounded-lg border p-3" style={{ borderColor: 'var(--border-subtle)' }}>
            {(
              [
                ['capabilities.connectors.oauthAuthorizeUrl', oauthAuthorizeUrl, setOauthAuthorizeUrl],
                ['capabilities.connectors.oauthTokenUrl', oauthTokenUrl, setOauthTokenUrl],
                ['capabilities.connectors.oauthClientId', oauthClientId, setOauthClientId],
                ['capabilities.connectors.oauthScopes', oauthScopes, setOauthScopes],
              ] as const
            ).map(([labelKey, value, setValue]) => (
              <input
                key={labelKey}
                value={value}
                onChange={(e) => setValue(e.target.value)}
                placeholder={t(locale, labelKey)}
                aria-label={t(locale, labelKey)}
                className="w-full rounded border px-3 py-2 font-mono text-xs"
                style={inputStyle}
              />
            ))}
            {isEdit ? (
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void handleAuthorize()}
                  disabled={!oauthConfigReady || oauthState === 'pending'}
                  title={!oauthConfigReady ? t(locale, 'capabilities.connectors.oauthConfigRequired') : undefined}
                  className="btn btn-ghost px-3 py-1.5 text-xs disabled:opacity-50"
                >
                  {t(locale, oauthState === 'success'
                    ? 'capabilities.connectors.oauthReauthorize'
                    : 'capabilities.connectors.oauthAuthorize')}
                </button>
                {oauthState === 'pending' ? (
                  <span className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                    {t(locale, 'capabilities.connectors.oauthPending')}
                  </span>
                ) : null}
                {oauthState === 'success' ? (
                  <span className="text-xs" style={{ color: 'var(--success)' }}>
                    {t(locale, 'capabilities.connectors.oauthSuccess')}
                  </span>
                ) : null}
                {oauthState === 'error' && oauthError ? (
                  <span className="text-xs" style={{ color: 'var(--danger)' }}>
                    {t(locale, 'capabilities.connectors.oauthFailed')}: {oauthError}
                  </span>
                ) : null}
              </div>
            ) : (
              <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'capabilities.connectors.secretCreateFirstSave')}
              </p>
            )}
          </div>
        ) : null}

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
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            className="px-4 py-2 text-sm disabled:opacity-50"
            style={{ color: 'var(--text-secondary)' }}
          >
            {t(locale, 'capabilities.common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={submitting}
            className="btn btn-primary px-4 py-2 text-sm disabled:opacity-50"
          >
            {t(locale, 'capabilities.common.save')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
