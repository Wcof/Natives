'use client';

import { useCallback, useEffect, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { ProviderSummary } from '@/types/provider';
import type { ProviderRoutingSettings } from '@/types/provider-routing';
import type { ProviderRouteBinding } from '@/types/provider-routing';
import { ProviderSettingsTabs, type ProviderSettingsTab } from './ProviderSettingsTabs';
import { RoutingPanel } from './RoutingPanel';
import { Sub2ApiAccountPool } from './Sub2ApiAccountPool';

export function ProviderSettingsWorkspace({ locale, providers, management, onProviderCreated }: { locale: Locale; providers: ProviderSummary[]; management: React.ReactNode; onProviderCreated: () => Promise<void> }) {
  const [tab, setTab] = useState<ProviderSettingsTab>('management');
  const [settings, setSettings] = useState<ProviderRoutingSettings | null>(null);
  const [bindings, setBindings] = useState<ProviderRouteBinding[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const api = typeof window === 'undefined' ? undefined : window.nativesAPI?.providerRouting;
  const load = useCallback(async () => {
    if (!api?.getSettings) { setError(t(locale, 'settings.routingUnavailable')); return; }
    setLoading(true); setError(null);
    try {
      setSettings(await api.getSettings());
      if (api.listBindings) setBindings(await api.listBindings());
    }
    catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setLoading(false); }
  }, [api, locale]);
  useEffect(() => { if (tab === 'routing') void load(); }, [load, tab]);
  const save = async (next: ProviderRoutingSettings) => {
    if (!api?.saveSettings) { setError(t(locale, 'settings.routingUnavailable')); return; }
    setSaving(true); setError(null);
    try { setSettings(await api.saveSettings(next)); }
    catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setSaving(false); }
  };
  const saveBindings = async (next: ProviderRouteBinding[]) => {
    if (!api?.saveBindings) { setError(t(locale, 'settings.routingUnavailable')); return; }
    setSaving(true); setError(null);
    try { setBindings(await api.saveBindings(next)); }
    catch (cause) { setError(classifyError(cause, { locale }).userMessage); }
    finally { setSaving(false); }
  };
  return <>
    <ProviderSettingsTabs locale={locale} activeTab={tab} onChange={setTab} />
    {tab === 'management' ? <>{management}<Sub2ApiAccountPool locale={locale} providers={providers} api={api} onProviderCreated={onProviderCreated} /></> : <RoutingPanel locale={locale} providers={providers} bindings={bindings} settings={settings} loading={loading} saving={saving} error={error} onSave={save} onSaveBindings={saveBindings} onRetry={() => void load()} />}
  </>;
}
