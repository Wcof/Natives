'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { KeyRound, Loader, Plus, ShieldCheck, Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import type { EnvProfileMetadata } from '@/lib/tauri/types-api';

export default function EnvironmentProfilesSettings({ locale }: { locale: Locale }) {
  const { toast } = useToast();
  const [profiles, setProfiles] = useState<EnvProfileMetadata[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<number | null>(null);
  const [profileName, setProfileName] = useState('');
  const [variableKey, setVariableKey] = useState('');
  const [secretValue, setSecretValue] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleteProfileTarget, setDeleteProfileTarget] = useState<EnvProfileMetadata | null>(null);
  const [deleteVariableTarget, setDeleteVariableTarget] = useState<string | null>(null);
  const generation = useRef(0);
  const reloadTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const loadProfiles = useCallback(async () => {
    const requestId = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.env) throw new Error('Environment profile API unavailable');
      const next = await api.env.listProfiles();
      if (requestId !== generation.current) return;
      setProfiles(next);
      setSelectedProfileId((current) =>
        current !== null && next.some((profile) => profile.id === current)
          ? current
          : next.find((profile) => profile.is_default === 1)?.id ?? next[0]?.id ?? null,
      );
    } catch (cause) {
      if (requestId !== generation.current) return;
      setProfiles([]);
      setSelectedProfileId(null);
      setError(classifyError(cause, { locale }).userMessage);
    } finally {
      if (requestId === generation.current) setLoading(false);
    }
  }, [locale]);

  useEffect(() => {
    void loadProfiles();
    const unsubscribe = window.nativesAPI?.onDbStateChanged?.((_event, channel) => {
      if (channel !== 'env') return;
      if (reloadTimer.current) clearTimeout(reloadTimer.current);
      reloadTimer.current = setTimeout(() => {
        reloadTimer.current = null;
        void loadProfiles();
      }, 80);
    });
    return () => {
      generation.current += 1;
      unsubscribe?.();
      if (reloadTimer.current) clearTimeout(reloadTimer.current);
      reloadTimer.current = null;
    };
  }, [loadProfiles]);

  const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId) ?? null;

  const createProfile = async () => {
    const name = profileName.trim();
    if (!name) {
      setError(t(locale, 'settings.environmentProfiles.validationProfile'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.env) throw new Error('Environment profile API unavailable');
      await api.env.createProfile(name);
      setProfileName('');
      toast(t(locale, 'settings.environmentProfiles.profileCreated'), 'success');
      await loadProfiles();
    } catch (cause) {
      setError(classifyError(cause, { locale }).userMessage);
    } finally {
      setBusy(false);
    }
  };

  const setDefault = async (profile: EnvProfileMetadata) => {
    setBusy(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.env) throw new Error('Environment profile API unavailable');
      await api.env.setDefaultProfile(profile.name);
      toast(t(locale, 'settings.environmentProfiles.defaultUpdated'), 'success');
      await loadProfiles();
    } catch (cause) {
      setError(classifyError(cause, { locale }).userMessage);
    } finally {
      setBusy(false);
    }
  };

  const deleteProfile = async () => {
    if (!deleteProfileTarget) return;
    setBusy(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.env) throw new Error('Environment profile API unavailable');
      await api.env.deleteProfile(deleteProfileTarget.name);
      setDeleteProfileTarget(null);
      toast(t(locale, 'settings.environmentProfiles.profileDeleted'), 'success');
      await loadProfiles();
    } catch (cause) {
      setError(classifyError(cause, { locale }).userMessage);
    } finally {
      setBusy(false);
    }
  };

  const addVariable = async () => {
    const key = variableKey.trim();
    if (!selectedProfile) return;
    if (!key) {
      setError(t(locale, 'settings.environmentProfiles.validationKey'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.env) throw new Error('Environment profile API unavailable');
      await api.env.setVariable(String(selectedProfile.id), key, secretValue);
      setVariableKey('');
      setSecretValue('');
      toast(t(locale, 'settings.environmentProfiles.variableAdded'), 'success');
      await loadProfiles();
    } catch (cause) {
      // Keep the secret draft on failure so a transient IPC error cannot erase user input.
      setError(classifyError(cause, { locale }).userMessage);
    } finally {
      setBusy(false);
    }
  };

  const deleteVariable = async () => {
    if (!selectedProfile || !deleteVariableTarget) return;
    setBusy(true);
    setError(null);
    try {
      const api = window.nativesAPI;
      if (!api?.env) throw new Error('Environment profile API unavailable');
      await api.env.deleteVariable(String(selectedProfile.id), deleteVariableTarget);
      setDeleteVariableTarget(null);
      toast(t(locale, 'settings.environmentProfiles.variableDeleted'), 'success');
      await loadProfiles();
    } catch (cause) {
      setError(classifyError(cause, { locale }).userMessage);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-section-card" aria-labelledby="environment-profiles-title">
      <div className="settings-section-heading settings-section-heading-with-icon">
        <span className="settings-preference-icon"><ShieldCheck size={18} /></span>
        <div>
          <h4 id="environment-profiles-title">{t(locale, 'settings.environmentProfiles.title')}</h4>
          <p>{t(locale, 'settings.environmentProfiles.description')}</p>
        </div>
      </div>

      {error && (
        <div role="alert" className="mb-3 flex items-center justify-between gap-3 rounded-md border border-[var(--danger)] p-3 text-sm text-[var(--danger)]">
          <span>{error}</span>
          <button type="button" className="btn-ghost text-xs" onClick={() => void loadProfiles()}>{t(locale, 'common.retry')}</button>
        </div>
      )}
      {loading ? (
        <div className="flex items-center gap-2 p-3 text-sm text-[var(--text-muted)]"><Loader size={14} className="animate-spin" />{t(locale, 'common.loading')}</div>
      ) : (
        <>
          <div className="flex flex-wrap items-end gap-2 border-b border-[var(--border)] pb-4">
            <label className="flex min-w-[220px] flex-1 flex-col gap-1 text-xs">
              {t(locale, 'settings.environmentProfiles.profileNameLabel')}
              <input
                className="settings-input"
                value={profileName}
                onChange={(event) => setProfileName(event.target.value)}
                disabled={busy}
                placeholder={t(locale, 'settings.environmentProfiles.profileNamePlaceholder')}
                maxLength={128}
                aria-label={t(locale, 'settings.environmentProfiles.profileNameLabel')}
              />
            </label>
            <button type="button" className="btn btn-primary inline-flex items-center gap-1.5" onClick={() => void createProfile()} disabled={busy}>
              <Plus size={13} />{busy ? t(locale, 'settings.environmentProfiles.creating') : t(locale, 'settings.environmentProfiles.createProfile')}
            </button>
          </div>

          {profiles.length === 0 ? (
            <p className="py-4 text-sm text-[var(--text-muted)]">{t(locale, 'settings.environmentProfiles.emptyProfiles')}</p>
          ) : (
            <div className="mt-4 grid gap-2">
              {profiles.map((profile) => (
                <div key={profile.id} className={`flex items-center gap-3 rounded-md border p-3 ${profile.id === selectedProfileId ? 'border-[var(--primary)]' : 'border-[var(--border)]'}`}>
                  <button
                    type="button"
                    className="min-w-0 flex-1 text-left"
                    onClick={() => setSelectedProfileId(profile.id)}
                    disabled={busy}
                    aria-pressed={profile.id === selectedProfileId}
                  >
                    <span className="font-medium">{profile.name}</span>
                    {profile.is_default === 1 && <span className="ml-2 text-xs text-[var(--primary)]">{t(locale, 'settings.environmentProfiles.defaultBadge')}</span>}
                  </button>
                  {profile.is_default !== 1 && <button type="button" className="btn-ghost text-xs" onClick={() => void setDefault(profile)} disabled={busy}>{t(locale, 'settings.environmentProfiles.makeDefault')}</button>}
                  <button
                    type="button"
                    className="btn-ghost text-[var(--danger)]"
                    onClick={() => setDeleteProfileTarget(profile)}
                    disabled={busy || profile.is_default === 1}
                    title={profile.is_default === 1 ? t(locale, 'settings.environmentProfiles.deleteProfileMessage') : undefined}
                    aria-label={`${t(locale, 'settings.environmentProfiles.deleteProfile')}: ${profile.name}`}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          )}

          {selectedProfile && (
            <div className="mt-5 border-t border-[var(--border)] pt-4">
              <div className="mb-3 flex items-center gap-2">
                <KeyRound size={15} />
                <h5 className="font-medium">{t(locale, 'settings.environmentProfiles.variablesTitle')} · {selectedProfile.name}</h5>
              </div>
              <p className="mb-3 text-xs text-[var(--text-muted)]">{t(locale, 'settings.environmentProfiles.metadataOnly')}</p>
              <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
                <label className="flex flex-col gap-1 text-xs">
                  {t(locale, 'settings.environmentProfiles.variableKeyLabel')}
                  <input className="settings-input" value={variableKey} onChange={(event) => setVariableKey(event.target.value)} disabled={busy} placeholder={t(locale, 'settings.environmentProfiles.variableKeyPlaceholder')} maxLength={256} autoCapitalize="characters" />
                </label>
                <label className="flex flex-col gap-1 text-xs">
                  {t(locale, 'settings.environmentProfiles.secretValueLabel')}
                  <input className="settings-input" type="password" autoComplete="new-password" value={secretValue} onChange={(event) => setSecretValue(event.target.value)} disabled={busy} placeholder={t(locale, 'settings.environmentProfiles.secretValuePlaceholder')} />
                </label>
                <button type="button" className="btn btn-primary self-end" onClick={() => void addVariable()} disabled={busy || !secretValue}><Plus size={13} /></button>
              </div>
              <div className="mt-3 grid gap-1">
                {selectedProfile.variables.length === 0 ? <p className="text-sm text-[var(--text-muted)]">{t(locale, 'settings.environmentProfiles.emptyVariables')}</p> : selectedProfile.variables.map((variable) => (
                  <div key={variable.key} className="flex items-center justify-between rounded-md border border-[var(--border)] px-3 py-2 text-sm">
                    <span className="font-mono">{variable.key}</span>
                    <span className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                      {variable.has_value ? t(locale, 'settings.environmentProfiles.maskedValue') : t(locale, 'settings.environmentProfiles.noValue')}
                      <button type="button" className="btn-ghost text-[var(--danger)]" onClick={() => setDeleteVariableTarget(variable.key)} disabled={busy} aria-label={`${t(locale, 'settings.environmentProfiles.deleteVariable')}: ${variable.key}`}><Trash2 size={13} /></button>
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </>
      )}

      <ConfirmDialog
        open={deleteProfileTarget !== null}
        title={t(locale, 'settings.environmentProfiles.deleteProfileTitle')}
        message={t(locale, 'settings.environmentProfiles.deleteProfileMessage')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => void deleteProfile()}
        onCancel={() => setDeleteProfileTarget(null)}
      />
      <ConfirmDialog
        open={deleteVariableTarget !== null}
        title={t(locale, 'settings.environmentProfiles.deleteVariableTitle')}
        message={t(locale, 'settings.environmentProfiles.deleteVariableMessage')}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => void deleteVariable()}
        onCancel={() => setDeleteVariableTarget(null)}
      />
    </section>
  );
}
