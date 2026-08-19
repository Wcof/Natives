'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ExternalLink, Loader2, LogIn, Unplug } from 'lucide-react';
import { BORDER_RADIUS, FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';

type OAuthAccountSummary = {
  id: string;
  name: string;
  platform: string;
  status: string;
  expiresAt: string | null;
  hasRefresh: boolean;
  projectId?: string | null;
  email?: string | null;
};

type OAuthFlow = 'pkce' | 'device';

type FixedOauthProvider = {
  id: string;
  name: string;
  flow: OAuthFlow;
  /** False while the client id is still a placeholder (login disabled). */
  configured: boolean;
};

const OAUTH_PROVIDERS: FixedOauthProvider[] = [
  { id: 'codex', name: 'Codex', flow: 'device', configured: true },
  { id: 'claude', name: 'Claude', flow: 'pkce', configured: false },
  { id: 'antigravity', name: 'Antigravity', flow: 'pkce', configured: true },
  { id: 'kimi', name: 'Kimi', flow: 'device', configured: false },
];

const POLL_INTERVAL_MS = 3000;

/**
 * Fixed OAuth provider cards (ADR-0019 P8). Codex / Claude / antigravity / Kimi
 * are shown as cards; each signs in via its catalog flow (device or PKCE) and
 * lists the connected account(s) with a disconnect action.
 */
export function ProviderOAuthAccounts({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const api = useMemo(
    () => (typeof window === 'undefined' ? undefined : window.nativesAPI?.providerOauth),
    [],
  );
  const [accounts, setAccounts] = useState<Record<string, OAuthAccountSummary[]>>({});
  const [loading, setLoading] = useState(true);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [device, setDevice] = useState<Record<string, { sessionId: string; userCode: string; verificationUri: string }>>({});
  const [confirmDisconnect, setConfirmDisconnect] = useState<{ providerId: string; accountId: string } | null>(null);
  const pollTimers = useRef<Record<string, number>>({});
  // Manual project_id fallback (antigravity): draft per account + saving flag.
  const [projectDrafts, setProjectDrafts] = useState<Record<string, string>>({});
  const [savingProject, setSavingProject] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!api) return;
    setLoading(true);
    const next: Record<string, OAuthAccountSummary[]> = {};
    for (const provider of OAUTH_PROVIDERS) {
      try {
        next[provider.id] = await api.status({ providerId: provider.id });
      } catch {
        next[provider.id] = [];
      }
    }
    setAccounts(next);
    setLoading(false);
  }, [api]);

  useEffect(() => {
    void load();
    const timers = pollTimers.current;
    return () => {
      Object.values(timers).forEach((id) => window.clearTimeout(id));
    };
  }, [load]);

  const stopDeviceFlow = useCallback((providerId: string) => {
    const timer = pollTimers.current[providerId];
    if (timer !== undefined) window.clearTimeout(timer);
    delete pollTimers.current[providerId];
    setDevice((cur) => {
      const { [providerId]: _drop, ...rest } = cur;
      return rest;
    });
  }, []);

  const pollDevice = useCallback(
    async (providerId: string, sessionId: string) => {
      if (!api) return;
      try {
        const result = await api.devicePoll({ sessionId });
        if (result.status === 'connected') {
          stopDeviceFlow(providerId);
          toast(t(locale, 'settings.providerOauthConnected'), 'success');
          await load();
          return;
        }
        if (result.status === 'expired') {
          stopDeviceFlow(providerId);
          toast(t(locale, 'settings.providerOauthError'), 'error');
          return;
        }
        pollTimers.current[providerId] = window.setTimeout(
          () => void pollDevice(providerId, sessionId),
          POLL_INTERVAL_MS,
        );
      } catch {
        stopDeviceFlow(providerId);
      }
    },
    [api, load, locale, stopDeviceFlow],
  );

  const startLogin = useCallback(
    async (provider: FixedOauthProvider) => {
      if (!api || !provider.configured) return;
      setConnecting(provider.id);
      try {
        if (provider.flow === 'pkce') {
          const result = await api.start({ providerId: provider.id });
          toast(
            result.hasRefresh
              ? t(locale, 'settings.providerOauthConnected')
              : t(locale, 'settings.providerOauthReauthRequired'),
            result.hasRefresh ? 'success' : 'warning',
          );
          await load();
        } else {
          const result = await api.deviceStart({ providerId: provider.id });
          setDevice((cur) => ({
            ...cur,
            [provider.id]: {
              sessionId: result.sessionId,
              userCode: result.userCode,
              verificationUri: result.verificationUri,
            },
          }));
          void pollDevice(provider.id, result.sessionId);
        }
      } catch (cause) {
        toast(classifyError(cause, { locale }).userMessage, 'error');
      } finally {
        setConnecting(null);
      }
    },
    [api, load, locale, pollDevice],
  );

  const disconnect = useCallback(async () => {
    if (!api || !confirmDisconnect) return;
    try {
      await api.disconnect({ providerId: confirmDisconnect.providerId, accountId: confirmDisconnect.accountId });
      await load();
      toast(t(locale, 'settings.providerOauthDisconnected'), 'success');
    } catch (cause) {
      toast(classifyError(cause, { locale }).userMessage, 'error');
    } finally {
      setConfirmDisconnect(null);
    }
  }, [api, confirmDisconnect, load, locale]);

  const saveProjectId = useCallback(
    async (providerId: string, accountId: string) => {
      if (!api) return;
      const draft = (projectDrafts[accountId] ?? '').trim();
      if (!draft) {
        toast(t(locale, 'settings.providerOauthProjectIdEmpty'), 'warning');
        return;
      }
      setSavingProject(accountId);
      try {
        await api.setProjectId({ providerId, accountId, projectId: draft });
        toast(t(locale, 'settings.providerOauthProjectIdSaved'), 'success');
        await load();
      } catch (cause) {
        toast(classifyError(cause, { locale }).userMessage, 'error');
      } finally {
        setSavingProject(null);
      }
    },
    [api, load, locale, projectDrafts],
  );

  if (!api) {
    return <div style={unavailableStyle}>{t(locale, 'settings.providerOauthLoading')}</div>;
  }

  return (
    <section className="settings-section-card" style={{ marginTop: SPACING.lg }}>
      <div style={headerStyle}>
        <div style={{ flex: 1 }}>
          <h3 style={titleStyle}>{t(locale, 'settings.providerOauthTitle')}</h3>
          <p style={descriptionStyle}>{t(locale, 'settings.providerOauthDesc')}</p>
        </div>
      </div>

      {loading ? (
        <div style={stateStyle}>{t(locale, 'settings.providerOauthLoading')}</div>
      ) : (
        <div style={gridStyle}>
          {OAUTH_PROVIDERS.map((provider) => {
            const list = accounts[provider.id] ?? [];
            const deviceState = device[provider.id];
            const isConnecting = connecting === provider.id;
            return (
              <div key={provider.id} style={cardStyle}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <strong style={{ color: 'var(--text)' }}>{provider.name}</strong>
                  {list.length > 0 ? (
                    list.map((account) => (
                      <div key={account.id} style={accountBlockStyle}>
                        <div style={accountRowStyle}>
                          <span style={secondaryStyle}>
                            {account.name || account.platform} ·{' '}
                            {account.hasRefresh
                              ? t(locale, 'settings.providerOauthConnected')
                              : t(locale, 'settings.providerOauthReauthRequired')}
                            {account.expiresAt && <> · {new Date(account.expiresAt).toLocaleString()}</>}
                          </span>
                          <button
                            type="button"
                            className="btn"
                            onClick={() => setConfirmDisconnect({ providerId: provider.id, accountId: account.id })}
                            style={{ color: 'var(--danger)' }}
                          >
                            <Unplug size={14} /> {t(locale, 'settings.providerOauthDisconnect')}
                          </button>
                        </div>
                        {provider.id === 'antigravity' && (
                          <div style={{ marginTop: SPACING.xs }}>
                            {account.projectId ? (
                              <span style={secondaryStyle}>
                                {t(locale, 'settings.providerOauthProjectId')}: {account.projectId}
                                {account.email ? ` · ${account.email}` : ''}
                              </span>
                            ) : (
                              <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.xs, flexWrap: 'wrap' }}>
                                <span style={secondaryStyle}>{t(locale, 'settings.providerOauthProjectIdMissing')}</span>
                                <input
                                  type="text"
                                  value={projectDrafts[account.id] ?? ''}
                                  onChange={(e) =>
                                    setProjectDrafts((cur) => ({ ...cur, [account.id]: e.target.value }))
                                  }
                                  placeholder={t(locale, 'settings.providerOauthProjectIdPlaceholder')}
                                  style={projectInputStyle}
                                />
                                <button
                                  type="button"
                                  className="btn"
                                  disabled={savingProject === account.id}
                                  onClick={() => void saveProjectId(provider.id, account.id)}
                                  style={{ whiteSpace: 'nowrap' }}
                                >
                                  {savingProject === account.id ? (
                                    <Loader2 size={14} className="animate-spin" />
                                  ) : (
                                    t(locale, 'settings.providerOauthSave')
                                  )}
                                </button>
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    ))
                  ) : deviceState ? (
                    <div style={{ marginTop: SPACING.sm }}>
                      <p style={secondaryStyle}>{t(locale, 'settings.providerOauthDeviceHint')}</p>
                      <div style={{ display: 'flex', alignItems: 'center', gap: SPACING.sm }}>
                        <code style={codeStyle}>{deviceState.userCode}</code>
                        <a href={deviceState.verificationUri} target="_blank" rel="noreferrer" style={{ display: 'inline-flex', alignItems: 'center', gap: 4, color: 'var(--primary)' }}>
                          {deviceState.verificationUri} <ExternalLink size={13} />
                        </a>
                      </div>
                    </div>
                  ) : (
                    <span style={secondaryStyle}>{t(locale, 'settings.providerOauthDisconnected')}</span>
                  )}
                </div>

                {list.length === 0 && (
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={isConnecting || !provider.configured}
                    onClick={() => void startLogin(provider)}
                    style={{ whiteSpace: 'nowrap' }}
                  >
                    {isConnecting ? (
                      <Loader2 size={14} className="animate-spin" />
                    ) : provider.configured ? (
                      <><LogIn size={14} /> {t(locale, 'settings.providerOauthLogin')}</>
                    ) : (
                      t(locale, 'settings.providerOauthInDevelopment')
                    )}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}

      <ConfirmDialog
        open={confirmDisconnect !== null}
        title={t(locale, 'settings.providerOauthDisconnect')}
        message={t(locale, 'settings.providerOauthDisconnectConfirm')}
        confirmLabel={t(locale, 'settings.providerOauthDisconnect')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => void disconnect()}
        onCancel={() => setConfirmDisconnect(null)}
      />
    </section>
  );
}

const headerStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'flex-start',
  gap: SPACING.md,
  marginBottom: SPACING.lg,
};
const titleStyle: React.CSSProperties = { margin: 0, color: 'var(--text)', fontSize: FONT_SIZE.md };
const descriptionStyle: React.CSSProperties = {
  margin: `${SPACING.xs}px 0 0`,
  color: 'var(--text-secondary)',
  fontSize: FONT_SIZE.xs,
  lineHeight: 1.5,
};
const gridStyle: React.CSSProperties = { display: 'grid', gap: SPACING.md };
const cardStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: SPACING.md,
  padding: SPACING.md,
  border: '1px solid var(--border)',
  borderRadius: BORDER_RADIUS.sm,
};
const accountRowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: SPACING.sm,
  marginTop: SPACING.xs,
};
const accountBlockStyle: React.CSSProperties = {
  borderTop: '1px solid var(--border)',
  paddingTop: SPACING.xs,
  marginTop: SPACING.xs,
};
const projectInputStyle: React.CSSProperties = {
  minWidth: 160,
  maxWidth: 260,
  padding: '3px 8px',
  border: '1px solid var(--border)',
  borderRadius: BORDER_RADIUS.xs,
  background: 'var(--surface)',
  color: 'var(--text)',
  fontFamily: 'var(--font-mono, monospace)',
  fontSize: FONT_SIZE.micro,
};
const secondaryStyle: React.CSSProperties = {
  display: 'block',
  color: 'var(--text-secondary)',
  fontSize: FONT_SIZE.micro,
};
const codeStyle: React.CSSProperties = {
  padding: '2px 8px',
  border: '1px solid var(--border)',
  borderRadius: BORDER_RADIUS.xs,
  background: 'var(--surface-hover)',
  color: 'var(--text)',
  fontFamily: 'var(--font-mono, monospace)',
  fontSize: FONT_SIZE.sm,
  letterSpacing: 1,
};
const stateStyle: React.CSSProperties = {
  padding: SPACING.xl,
  color: 'var(--text-secondary)',
  fontSize: FONT_SIZE.sm,
  textAlign: 'center',
};
const unavailableStyle: React.CSSProperties = {
  marginTop: SPACING.lg,
  padding: SPACING.lg,
  border: '1px dashed var(--border)',
  borderRadius: BORDER_RADIUS.md,
  color: 'var(--text-secondary)',
  fontSize: FONT_SIZE.sm,
};
