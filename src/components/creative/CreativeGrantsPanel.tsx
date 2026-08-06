//! Creative App permission panel (T08).
//!
//! Grants (upload / download / clipboard / window_open), the grant lifecycle
//! history, the OAuth allowlist and the app's profile binding. Every Host call
//! has explicit loading / error / success states; errors go through the
//! classifier (R-F5). Grants default to deny — the panel never fabricates a
//! grant; it reads the Host store.

import React from 'react';
import { ShieldAlert, History, X } from 'lucide-react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import {
  eventLabelKey,
  grantPolicyFor,
  kindLabelKey,
  policyLabelKey,
  sortGrantEvents,
  sortGrants,
} from '@/lib/creative-grants';
import type {
  AppGrant,
  BrowserProfile,
  GrantEvent,
  OAuthAllowlistEntry,
  ProfileBinding,
} from '@/lib/tauri-adapter';

export interface CreativeGrantsPanelProps {
  appId: string;
  appTitle: string;
  onClose: () => void;
  onToast: (message: string) => void;
}

export default function CreativeGrantsPanel({
  appId,
  appTitle,
  onClose,
  onToast,
}: CreativeGrantsPanelProps) {
  const locale = useLocale();
  const [profiles, setProfiles] = React.useState<BrowserProfile[]>([]);
  const [binding, setBinding] = React.useState<ProfileBinding | null>(null);
  const [grants, setGrants] = React.useState<AppGrant[]>([]);
  const [history, setHistory] = React.useState<GrantEvent[]>([]);
  const [oauthDomains, setOauthDomains] = React.useState<OAuthAllowlistEntry[]>([]);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);

  const load = React.useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const api = window.nativesAPI?.creativeApp;
      if (!api) throw new Error('creativeApp adapter unavailable');
      const [p, b, g, h, d] = await Promise.all([
        api.profileList(),
        api.profileBindings(),
        api.grantList(appId),
        api.grantEvents(appId),
        api.oauthDomains(appId),
      ]);
      setProfiles(p);
      setBinding(b.find((x) => x.applicationId === appId) ?? null);
      setGrants(sortGrants(g));
      setHistory(sortGrantEvents(h));
      setOauthDomains(d);
    } catch (err) {
      setError(classifyError(err).userMessage);
    } finally {
      setLoading(false);
    }
  }, [appId]);

  React.useEffect(() => {
    void load();
  }, [load]);

  const run = async (key: string, fn: () => Promise<unknown>, successKey?: string) => {
    setBusy(key);
    setError(null);
    try {
      await fn();
      if (successKey) onToast(t(locale, successKey));
      await load();
    } catch (err) {
      setError(classifyError(err).userMessage);
    } finally {
      setBusy(null);
    }
  };

  const grantAction = (kind: string, policy: string) =>
    run(
      `grant-${kind}`,
      () => window.nativesAPI!.creativeApp.grantSet(appId, kind, policy, null),
      'creative.grant.setSuccess',
    );

  return (
    <div className="flex flex-col h-full bg-[var(--background)]">
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-[var(--border)] bg-[var(--surface)] shrink-0">
        <ShieldAlert size={14} className="text-[var(--text-secondary)]" />
        <span className="flex-1 text-xs font-medium truncate">
          {t(locale, 'creative.grant.title')} · {appTitle}
        </span>
        <button
          type="button"
          className="flex h-7 w-7 items-center justify-center rounded-lg border border-[var(--border)] hover:bg-[var(--surface-hover)]"
          onClick={onClose}
          title={t(locale, 'workshop.browserBackToList')}
        >
          <X size={13} />
        </button>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto px-4 py-3 flex flex-col gap-4">
        {loading && (
          <div className="text-xs text-[var(--text-secondary)]">
            {t(locale, 'creative.grant.loading')}
          </div>
        )}
        {error && (
          <div className="text-xs text-[var(--danger)] bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg px-3 py-2">
            {error}
          </div>
        )}
        {!loading && !error && (
          <>
            <section className="flex flex-col gap-1.5">
              <h3 className="text-[11px] font-medium text-[var(--text-secondary)]">
                {t(locale, 'creative.grant.title')}
              </h3>
              <div className="flex flex-col gap-1.5">
                {['upload', 'download', 'clipboard', 'window_open'].map((kind) => {
                  const policy = grantPolicyFor(grants, kind);
                  return (
                    <div
                      key={kind}
                      className="flex items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2"
                    >
                      <span className="flex-1 text-xs">{t(locale, kindLabelKey(kind))}</span>
                      <span className="text-[11px] text-[var(--text-secondary)]">
                        {t(locale, policyLabelKey(policy))}
                      </span>
                      {policy !== 'persistent' && (
                        <button
                          type="button"
                          disabled={busy === `grant-${kind}`}
                          className="h-6 px-2 rounded text-[11px] border border-[var(--border)] disabled:opacity-50"
                          onClick={() => void grantAction(kind, 'persistent')}
                        >
                          {t(locale, 'creative.grant.grant')}
                        </button>
                      )}
                      {policy !== 'default_deny' && (
                        <button
                          type="button"
                          disabled={busy === `grant-${kind}`}
                          className="h-6 px-2 rounded text-[11px] border border-[var(--border)] text-[var(--danger)] disabled:opacity-50"
                          onClick={() => {
                            const g = grants.find((x) => x.kind === kind);
                            if (g) void run(`revoke-${kind}`, () => window.nativesAPI!.creativeApp.grantDelete(g.id), 'creative.grant.revoked');
                          }}
                        >
                          {t(locale, 'creative.grant.revoke')}
                        </button>
                      )}
                    </div>
                  );
                })}
                <div className="text-[11px] text-[var(--text-secondary)]">
                  {t(locale, 'creative.grant.defaultDenyHint')}
                </div>
              </div>
            </section>

            <section className="flex flex-col gap-1.5">
              <h3 className="text-[11px] font-medium text-[var(--text-secondary)]">
                {t(locale, 'creative.profile.title')}
              </h3>
              <div className="flex items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2">
                <select
                  className="flex-1 h-7 text-xs rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                  value={binding?.profileId ?? ''}
                  onChange={(e) => {
                    const profileId = e.target.value;
                    if (!profileId) {
                      void run('unbind', () => window.nativesAPI!.creativeApp.profileUnbind(appId));
                    } else {
                      void run('bind', () => window.nativesAPI!.creativeApp.profileBind(appId, profileId));
                    }
                  }}
                >
                  {!binding && <option value="">{t(locale, 'creative.profile.defaultBadge')}</option>}
                  {profiles.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                      {p.isDefault ? ` · ${t(locale, 'creative.profile.defaultBadge')}` : ''}
                    </option>
                  ))}
                </select>
                <span className="text-[11px] text-[var(--text-secondary)]">
                  {t(locale, 'creative.profile.bindHint')}
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <input
                  data-testid="new-profile-name"
                  className="flex-1 h-7 px-2 text-xs rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                  placeholder={t(locale, 'creative.profile.createPlaceholder')}
                  onKeyDown={(e) => {
                    if (e.key !== 'Enter') return;
                    const name = e.currentTarget.value.trim();
                    if (!name) return;
                    e.currentTarget.value = '';
                    void run('create-profile', () => window.nativesAPI!.creativeApp.profileCreate(name), 'creative.profile.createSuccess');
                  }}
                />
                <button
                  type="button"
                  disabled={busy === 'create-profile'}
                  className="h-7 px-2 rounded text-[11px] border border-[var(--border)] disabled:opacity-50"
                  onClick={() => {
                    const input = document.querySelector<HTMLInputElement>('[data-testid="new-profile-name"]');
                    const name = input?.value.trim() ?? '';
                    if (!name) return;
                    if (input) input.value = '';
                    void run('create-profile', () => window.nativesAPI!.creativeApp.profileCreate(name), 'creative.profile.createSuccess');
                  }}
                >
                  {t(locale, 'creative.profile.create')}
                </button>
              </div>
            </section>

            {oauthDomains.length > 0 && (
              <section className="flex flex-col gap-1.5">
                <h3 className="text-[11px] font-medium text-[var(--text-secondary)]">
                  {t(locale, 'creative.oauth.allowlist')}
                </h3>
                <div className="flex flex-wrap gap-1">
                  {oauthDomains.map((d) => (
                    <span
                      key={d.id}
                      className="text-[11px] rounded-full border border-[var(--border)] px-2 py-0.5 bg-[var(--surface)]"
                    >
                      {d.domain}
                    </span>
                  ))}
                </div>
              </section>
            )}

            <section className="flex flex-col gap-1.5">
              <h3 className="flex items-center gap-1.5 text-[11px] font-medium text-[var(--text-secondary)]">
                <History size={11} />
                {t(locale, 'creative.grant.historyTitle')}
              </h3>
              {history.length === 0 ? (
                <div className="text-[11px] text-[var(--text-secondary)]">
                  {t(locale, 'creative.grant.historyEmpty')}
                </div>
              ) : (
                <div className="flex flex-col gap-1">
                  {history.map((e) => (
                    <div
                      key={e.id}
                      className="flex items-center gap-2 rounded border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-[11px]"
                    >
                      <span className="text-[var(--text-secondary)]">
                        {t(locale, kindLabelKey(e.kind))}
                      </span>
                      <span>{t(locale, eventLabelKey(e.event))}</span>
                      {e.path && (
                        <span className="text-[var(--text-secondary)] truncate max-w-[180px]">
                          {e.path}
                        </span>
                      )}
                      <span className="flex-1 text-right text-[var(--text-secondary)]">
                        {new Date(e.createdAt).toLocaleTimeString(locale)}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </>
        )}
      </div>
    </div>
  );
}
