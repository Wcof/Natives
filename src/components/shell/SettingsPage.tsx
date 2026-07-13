'use client';

import { useState, useEffect } from 'react';
import { normalizeThemeId, applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { Trash2 } from 'lucide-react';
import RuntimePanel from '@/components/assistant/RuntimePanel';
import ProviderDetail from '@/components/settings/ProviderDetail';
import AddProviderDialog from '@/components/settings/AddProviderDialog';
import type { ProviderSummary, TestKeyResult } from '@/types/provider';

export default function SettingsPage() {
  const { toast: globalToast } = useToast();
  const [locale, setLocaleState] = useState<Locale>('zh');
  const [activeTab, setActiveTab] = useState<'theme' | 'env' | 'plugins' | 'executor' | 'providers'>('theme');
  const TABS = [
    { id: 'theme' as const, label: t(locale, 'settings.tabTheme') },
    { id: 'env' as const, label: t(locale, 'settings.tabEnv') },
    { id: 'plugins' as const, label: t(locale, 'settings.tabPlugins') },
    { id: 'executor' as const, label: t(locale, 'settings.tabExecutor') },
    { id: 'providers' as const, label: t(locale, 'settings.tabProviders') },
  ];

  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [providersLoading, setProvidersLoading] = useState(false);
  const [deleteProviderTarget, setDeleteProviderTarget] = useState<string | null>(null);

  useEffect(() => { if (activeTab === 'providers') loadProviders(); }, [activeTab]);

  useEffect(() => {
    (async () => {
      try {
        const api = window.nativesAPI;
        if (!api) return;
        const sl = await api.getLocale().catch(() => null);
        if (sl) setLocaleState(sl as Locale);
      } catch { /* noop */ }
    })();
  }, []);

  async function loadProviders() {
    setProvidersLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.provider?.unifiedList) setProviders(Array.isArray(await api.provider.unifiedList()) ? await api.provider.unifiedList() as ProviderSummary[] : []);
      else if (api?.provider?.list) setProviders(Array.isArray(await api.provider.list()) ? await api.provider.list() as ProviderSummary[] : []);
    } catch (e) { globalToast(classifyError(e).userMessage, 'error'); }
    finally { setProvidersLoading(false); }
  }

  async function handleSaveProvider(data: { presetName: string; name: string; websiteUrl: string; baseUrl: string; keys: { label: string; apiKey: string }[] }) {
    const api = window.nativesAPI;
    if (api?.provider?.create) {
      await api.provider.create({ providerType: data.presetName, displayName: data.name, websiteUrl: data.websiteUrl, baseUrl: data.baseUrl, initialKey: data.keys[0] ? { label: data.keys[0].label, apiKey: data.keys[0].apiKey } : null });
    } else if (api?.provider?.add) { await api.provider.add(data); }
    else throw new Error('Provider API not available');
    globalToast(t(locale, 'settings.providerAdded'), 'success');
    await loadProviders();
  }

  async function handleDeleteProvider(id: string) { if (!window.nativesAPI?.provider?.delete) return; await window.nativesAPI.provider.delete(id); globalToast(t(locale, 'settings.providerDeleted'), 'success'); await loadProviders(); }
  async function handleSaveDefaults(pid: string, m: string | null) { const a = window.nativesAPI; if (a?.provider?.updateDefaults) { await a.provider.updateDefaults({ providerId: pid, defaultModel: m }); await loadProviders(); } else throw new Error('updateDefaults'); }
  async function handleAddKey(pid: string, l: string, k: string) { const a = window.nativesAPI; if (a?.provider?.addKeyUnified) await a.provider.addKeyUnified({ providerId: pid, label: l, apiKey: k }); else if (a?.provider?.addKey) await a.provider.addKey({ providerId: pid, label: l, apiKey: k }); else throw new Error('addKey'); await loadProviders(); }
  async function handleTestKey(pid: string, kid: string): Promise<TestKeyResult> { const a = window.nativesAPI; if (a?.provider?.testKey) { const r = await a.provider.testKey({ providerId: pid, keyId: kid }); await loadProviders(); return r as unknown as TestKeyResult; } if (a?.provider?.test) { const r = await a.provider.test({ providerId: pid, keyId: kid }); await loadProviders(); return { success: r.success, status: r.success ? 'valid' as const : 'invalid' as const, testedAt: new Date().toISOString(), errorCode: r.success ? null : 'unknown', userMessage: r.error || null }; } throw new Error('testKey'); }
  async function handleSetPrimaryKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.setPrimaryKey) { await a.provider.setPrimaryKey({ providerId: pid, keyId: kid }); await loadProviders(); } else throw new Error('setPrimaryKey'); }
  async function handleDeleteKey(pid: string, kid: string) { const a = window.nativesAPI; if (a?.provider?.deleteKeyUnified) await a.provider.deleteKeyUnified({ providerId: pid, keyId: kid }); else if (a?.provider?.deleteKey) await a.provider.deleteKey(kid); else throw new Error('deleteKey'); await loadProviders(); }

  const btnStyle: React.CSSProperties = { flex: 1, textAlign: 'center', padding: '7px 4px', borderRadius: 'calc(var(--radius-sm) - 2px)', cursor: 'pointer', border: '1px solid transparent', fontSize: FONT_SIZE.md, transition: 'all 0.12s' };

  return (
    <div style={{height:'100%',overflow:'auto'}}>
      <div style={{padding:`${SPACING.lg}px 20px`,display:'flex',flexDirection:'column',gap:28}}>
        <div style={{display:'flex',gap:4,background:'var(--surface-hover)',border:'1px solid var(--border)',borderRadius:'var(--radius-sm)',padding:3}}>
          {TABS.map(tab => {
            const isActive = activeTab === tab.id;
            return <button key={tab.id} onClick={() => setActiveTab(tab.id)}
              style={{...btnStyle, background: isActive ? 'var(--primary-soft)' : 'transparent', color: isActive ? 'var(--primary)' : 'var(--text-secondary)', border: isActive ? '1px solid var(--primary)' : '1px solid transparent', fontWeight: isActive ? 600 : 400 }}>
              {tab.label}
            </button>;
          })}
        </div>
        <div style={{ display: activeTab === 'theme' ? 'block' : 'none' }}><h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600 }}>{t(locale, 'settings.theme')}</h2></div>
        <div style={{ display: activeTab === 'env' ? 'block' : 'none' }}><h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600 }}>{t(locale, 'settings.environment')}</h2></div>
        <div style={{ display: activeTab === 'plugins' ? 'block' : 'none' }}><h2 style={{ fontSize: FONT_SIZE.lg, fontWeight: 600 }}>{t(locale, 'settings.tabPlugins')}</h2></div>
        <div style={{ display: activeTab === 'executor' ? 'block' : 'none' }}><RuntimePanel locale={locale} /></div>
        <div style={{ display: activeTab === 'providers' ? 'block' : 'none' }}>
          <ProviderDetail
            locale={locale} providers={providers} loading={providersLoading}
            showAddProvider={() => setShowAddProvider(true)}
            onSaveDefaults={handleSaveDefaults} onAddKey={handleAddKey} onTestKey={handleTestKey}
            onSetPrimaryKey={handleSetPrimaryKey} onDeleteKey={handleDeleteKey}
            onDeleteProvider={(id) => setDeleteProviderTarget(id)}
          />
        </div>
      </div>
      {showAddProvider && <AddProviderDialog locale={locale} onClose={() => setShowAddProvider(false)} onSave={handleSaveProvider} />}
      <ConfirmDialog open={deleteProviderTarget !== null} title={t(locale, 'settings.deleteProvider')} message={t(locale, 'settings.confirmDeleteProvider')} confirmLabel={t(locale, 'common.delete')} cancelLabel={t(locale, 'common.cancel')} danger
        onConfirm={() => { if (deleteProviderTarget) handleDeleteProvider(deleteProviderTarget); setDeleteProviderTarget(null); }}
        onCancel={() => setDeleteProviderTarget(null)} />
    </div>
  );
}
