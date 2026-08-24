'use client';

import { useState, useEffect, useCallback } from 'react';
import {
  aiApi,
  type Provider,
  type Connection,
  type Credential,
  type Model,
  type QuotaSnapshot,
  type AiResourcesSummary,
  type DeleteImpact,
} from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import {
  ShieldCheck,
  Plus,
  Trash2,
  Activity,
  Key,
  RefreshCw,
  CheckCircle2,
  AlertCircle,
  ExternalLink,
  Search,
  Sparkles,
  Zap,
} from 'lucide-react';
import { AddProviderModal } from './resources/AddProviderModal';
import { AddConnectionModal } from './resources/AddConnectionModal';
import { AddApiKeyModal } from './resources/AddApiKeyModal';
import { OAuthConnectModal } from './resources/OAuthConnectModal';
import { DiscoverModelsModal } from './resources/DiscoverModelsModal';
import { ManualModelModal } from './resources/ManualModelModal';
import { DeleteImpactModal } from './resources/DeleteImpactModal';

export default function AiResourcesPanel() {
  const locale = useLocale();
  const [summary, setSummary] = useState<AiResourcesSummary | null>(null);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<Provider | null>(null);
  const [activeTab, setActiveTab] = useState<'overview' | 'connections' | 'credentials' | 'models' | 'quota'>('overview');

  const [connections, setConnections] = useState<Connection[]>([]);
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  const [quotas, setQuotas] = useState<Record<string, QuotaSnapshot | null>>({});
  const [searchQuery, setSearchQuery] = useState('');
  const [healthStatus, setHealthStatus] = useState<Record<string, { reachable: boolean; error: string | null }>>({});

  // Modals
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [showAddConnection, setShowAddConnection] = useState(false);
  const [showAddApiKey, setShowAddApiKey] = useState(false);
  const [showOAuthModal, setShowOAuthModal] = useState(false);
  const [showDiscoverModels, setShowDiscoverModels] = useState(false);
  const [showManualModel, setShowManualModel] = useState(false);
  const [deleteImpact, setDeleteImpact] = useState<DeleteImpact | null>(null);
  const [deletingProviderId, setDeletingProviderId] = useState<string | null>(null);

  const loadSummaryAndProviders = useCallback(async () => {
    try {
      const [sum, list] = await Promise.all([aiApi.getSummary(), aiApi.listProviders()]);
      setSummary(sum);
      setProviders(list);
      if (list.length > 0 && (!selectedProvider || !list.some((p) => p.id === selectedProvider.id))) {
        setSelectedProvider(list[0] || null);
      }
    } catch (e) {
      console.error('Failed to load AI resources:', e);
    }
  }, [selectedProvider]);

  const loadProviderDetails = useCallback(async (providerId: string) => {
    try {
      const [conns, creds] = await Promise.all([
        aiApi.listConnections(providerId),
        aiApi.listCredentials(providerId),
      ]);
      setConnections(conns);
      setCredentials(creds);

      if (conns.length > 0 && conns[0]) {
        const m = await aiApi.listModels(conns[0].id);
        setModels(m);
      } else {
        const m = await aiApi.listModels(undefined, undefined);
        setModels(m.filter((item) => item.providerId === providerId));
      }

      const qMap: Record<string, QuotaSnapshot | null> = {};
      for (const cred of creds) {
        if (cred.kind === 'oauth') {
          const q = await aiApi.getQuota(cred.id);
          qMap[cred.id] = q;
        }
      }
      setQuotas(qMap);
    } catch (e) {
      console.error('Failed to load provider details:', e);
    }
  }, []);

  useEffect(() => {
    void loadSummaryAndProviders();
  }, [loadSummaryAndProviders]);

  useEffect(() => {
    if (selectedProvider) {
      void loadProviderDetails(selectedProvider.id);
    }
  }, [selectedProvider, loadProviderDetails]);

  const handleHealthCheck = async (conn: Connection) => {
    try {
      const res = await aiApi.checkHealth(conn.baseUrl);
      setHealthStatus((prev) => ({ ...prev, [conn.id]: res }));
    } catch (e) {
      setHealthStatus((prev) => ({ ...prev, [conn.id]: { reachable: false, error: String(e) } }));
    }
  };

  const requestDeleteProvider = async (provider: Provider) => {
    setDeletingProviderId(provider.id);
    try {
      const impact = await aiApi.getProviderDeleteImpact(provider.id);
      setDeleteImpact(impact);
    } catch (err) {
      console.error(err);
      setDeleteImpact({ connectionCount: 0, credentialCount: 0, modelCount: 0, affectedRouteCount: 0 });
    }
  };

  const confirmDeleteProvider = async () => {
    if (!deletingProviderId) return;
    try {
      await aiApi.deleteProvider(deletingProviderId);
      setDeletingProviderId(null);
      setDeleteImpact(null);
      await loadSummaryAndProviders();
    } catch (err) {
      console.error(err);
    }
  };

  const handleDeleteCredential = async (credId: string) => {
    if (!selectedProvider) return;
    try {
      await aiApi.deleteCredential({ providerId: selectedProvider.id, credentialId: credId });
      await loadProviderDetails(selectedProvider.id);
      await loadSummaryAndProviders();
    } catch (err) {
      console.error(err);
    }
  };

  const filteredProviders = providers.filter(
    (p) =>
      p.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      p.websiteUrl.toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <div className="flex flex-col h-full bg-[var(--background)] text-[var(--foreground)]">
      {/* Top Header & Summary Stats */}
      <div className="p-6 border-b border-[var(--border)] bg-[var(--card)]/40 backdrop-blur-md">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h1 className="text-2xl font-bold flex items-center gap-2 font-display text-[var(--foreground)]">
              <ShieldCheck className="w-7 h-7 text-[var(--primary)]" />
              {t(locale, 'aiResources.title')}
            </h1>
            <p className="text-sm text-[var(--muted-foreground)] mt-1">
              {t(locale, 'aiResources.desc')}
            </p>
          </div>
          <div className="flex items-center gap-3">
            <button
              onClick={() => setShowAddProvider(true)}
              className="px-4 py-2 bg-[var(--secondary)] hover:bg-[var(--secondary)]/80 text-[var(--secondary-foreground)] rounded-lg font-medium text-sm flex items-center gap-2 transition"
            >
              <Plus className="w-4 h-4" />
              {t(locale, 'aiResources.addProvider')}
            </button>
            <button
              onClick={() => setShowOAuthModal(true)}
              className="px-4 py-2 bg-[var(--primary)] hover:bg-[var(--primary)]/90 text-[var(--primary-foreground)] rounded-lg font-medium text-sm flex items-center gap-2 shadow-sm transition"
            >
              <Zap className="w-4 h-4" />
              {t(locale, 'aiResources.connectOauth')}
            </button>
          </div>
        </div>

        {/* 4 Stat Tiles */}
        <div className="grid grid-cols-4 gap-4 mt-2">
          <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] shadow-sm">
            <div className="text-xs text-[var(--muted-foreground)] uppercase font-semibold">
              {t(locale, 'aiResources.statProviders')}
            </div>
            <div className="text-2xl font-bold mt-1 text-[var(--foreground)]">
              {summary?.providerCount ?? providers.length}
            </div>
          </div>
          <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] shadow-sm">
            <div className="text-xs text-[var(--muted-foreground)] uppercase font-semibold">
              {t(locale, 'aiResources.statConnections')}
            </div>
            <div className="text-2xl font-bold mt-1 text-[var(--foreground)]">
              {summary?.connectionCount ?? 0}
            </div>
          </div>
          <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] shadow-sm">
            <div className="text-xs text-[var(--muted-foreground)] uppercase font-semibold">
              {t(locale, 'aiResources.statCredentials')}
            </div>
            <div className="text-2xl font-bold mt-1 text-[var(--primary)]">
              {summary?.credentialCount ?? 0}
            </div>
          </div>
          <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] shadow-sm">
            <div className="text-xs text-[var(--muted-foreground)] uppercase font-semibold">
              {t(locale, 'aiResources.statModels')}
            </div>
            <div className="text-2xl font-bold mt-1 text-[var(--foreground)]">
              {summary?.availableModelCount ?? 0}
            </div>
          </div>
        </div>
      </div>

      {/* Main Two-Column View */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Column: Provider List */}
        <div className="w-80 border-r border-[var(--border)] flex flex-col bg-[var(--card)]/20">
          <div className="p-3 border-b border-[var(--border)]">
            <div className="relative">
              <Search className="w-4 h-4 absolute left-3 top-2.5 text-[var(--muted-foreground)]" />
              <input
                type="text"
                placeholder={t(locale, 'aiResources.searchPlaceholder')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-9 pr-3 py-1.5 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
              />
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {filteredProviders.length === 0 ? (
              <div className="p-6 text-center text-sm text-[var(--muted-foreground)]">
                {t(locale, 'aiResources.noProviders')}
              </div>
            ) : (
              filteredProviders.map((p) => {
                const isSelected = selectedProvider?.id === p.id;
                return (
                  <button
                    key={p.id}
                    onClick={() => setSelectedProvider(p)}
                    className={`w-full text-left p-3 rounded-xl transition flex items-center justify-between ${
                      isSelected
                        ? 'bg-[var(--primary)]/10 border border-[var(--primary)]/30 text-[var(--foreground)]'
                        : 'hover:bg-[var(--card)]/80 text-[var(--foreground)] border border-transparent'
                    }`}
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      <div className="w-9 h-9 rounded-lg bg-[var(--secondary)] flex items-center justify-center font-bold text-sm text-[var(--secondary-foreground)] uppercase">
                        {p.name.slice(0, 2)}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="font-semibold text-sm truncate">{p.name}</div>
                        <div className="text-xs text-[var(--muted-foreground)] truncate">{p.websiteUrl || 'No URL'}</div>
                      </div>
                    </div>
                    {p.presetKey && (
                      <span className="text-[10px] uppercase font-bold px-2 py-0.5 rounded bg-[var(--secondary)] text-[var(--secondary-foreground)]">
                        {p.presetKey}
                      </span>
                    )}
                  </button>
                );
              })
            )}
          </div>
        </div>

        {/* Right Column: Selected Provider Details */}
        <div className="flex-1 flex flex-col overflow-y-auto bg-[var(--background)]">
          {selectedProvider ? (
            <div className="p-6 space-y-6">
              {/* Provider Detail Header */}
              <div className="flex items-center justify-between p-4 rounded-xl bg-[var(--card)] border border-[var(--border)]">
                <div>
                  <div className="flex items-center gap-3">
                    <h2 className="text-xl font-bold font-display">{selectedProvider.name}</h2>
                    {selectedProvider.presetKey && (
                      <span className="text-xs px-2.5 py-0.5 rounded-full bg-[var(--primary)]/10 text-[var(--primary)] border border-[var(--primary)]/20 font-medium">
                        Preset: {selectedProvider.presetKey}
                      </span>
                    )}
                  </div>
                  {selectedProvider.websiteUrl && (
                    <a
                      href={selectedProvider.websiteUrl}
                      target="_blank"
                      rel="noreferrer"
                      className="text-xs text-[var(--muted-foreground)] hover:text-[var(--primary)] flex items-center gap-1 mt-1 transition"
                    >
                      {selectedProvider.websiteUrl}
                      <ExternalLink className="w-3 h-3" />
                    </a>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => requestDeleteProvider(selectedProvider)}
                    className="p-2 text-[var(--destructive)] hover:bg-[var(--destructive)]/10 rounded-lg transition"
                    title={t(locale, 'aiResources.deleteProvider')}
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              </div>

              {/* Tabs navigation */}
              <div className="flex gap-2 border-b border-[var(--border)] pb-2">
                {(['overview', 'connections', 'credentials', 'models', 'quota'] as const).map((tab) => (
                  <button
                    key={tab}
                    onClick={() => setActiveTab(tab)}
                    className={`px-4 py-2 text-sm font-medium rounded-lg transition capitalize ${
                      activeTab === tab
                        ? 'bg-[var(--primary)] text-[var(--primary-foreground)] shadow-sm'
                        : 'text-[var(--muted-foreground)] hover:bg-[var(--card)]'
                    }`}
                  >
                    {tab === 'overview' && t(locale, 'aiResources.overview')}
                    {tab === 'connections' && t(locale, 'aiResources.connections')}
                    {tab === 'credentials' && t(locale, 'aiResources.credentials')}
                    {tab === 'models' && t(locale, 'aiResources.models')}
                    {tab === 'quota' && t(locale, 'aiResources.quota')}
                  </button>
                ))}
              </div>

              {/* Tab Contents */}
              {activeTab === 'overview' && (
                <div className="space-y-4">
                  <div className="grid grid-cols-3 gap-4">
                    <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)]">
                      <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'aiResources.connections')}</div>
                      <div className="text-xl font-bold mt-1">{connections.length}</div>
                    </div>
                    <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)]">
                      <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'aiResources.credentials')}</div>
                      <div className="text-xl font-bold mt-1 text-[var(--primary)]">{credentials.length}</div>
                    </div>
                    <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)]">
                      <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'aiResources.models')}</div>
                      <div className="text-xl font-bold mt-1">{models.length}</div>
                    </div>
                  </div>
                </div>
              )}

              {activeTab === 'connections' && (
                <div className="space-y-4">
                  <div className="flex justify-between items-center">
                    <h3 className="font-bold text-sm text-[var(--muted-foreground)] uppercase">
                      {t(locale, 'aiResources.upstreamConnections')}
                    </h3>
                    <button
                      onClick={() => setShowAddConnection(true)}
                      className="px-3 py-1.5 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-xs font-semibold flex items-center gap-1.5"
                    >
                      <Plus className="w-3.5 h-3.5" />
                      {t(locale, 'aiResources.addConnection')}
                    </button>
                  </div>

                  <div className="space-y-3">
                    {connections.length === 0 ? (
                      <div className="p-8 text-center text-sm text-[var(--muted-foreground)] border border-dashed border-[var(--border)] rounded-xl">
                        {t(locale, 'aiResources.noConnections')}
                      </div>
                    ) : (
                      connections.map((conn) => {
                        const health = healthStatus[conn.id];
                        return (
                          <div
                            key={conn.id}
                            className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] flex items-center justify-between"
                          >
                            <div className="space-y-1">
                              <div className="font-semibold text-sm flex items-center gap-2">
                                {conn.name}
                                <span className="text-[10px] uppercase font-bold px-2 py-0.5 rounded bg-[var(--secondary)] text-[var(--secondary-foreground)]">
                                  {conn.upstreamProtocol}
                                </span>
                              </div>
                              <div className="text-xs text-[var(--muted-foreground)] font-mono">{conn.baseUrl}</div>
                              {health && (
                                <div className="flex items-center gap-1.5 text-xs mt-2">
                                  {health.reachable ? (
                                    <span className="text-[var(--success)] flex items-center gap-1 font-medium">
                                      <CheckCircle2 className="w-3.5 h-3.5" />
                                      {t(locale, 'aiResources.reachable')}
                                    </span>
                                  ) : (
                                    <span className="text-[var(--destructive)] flex items-center gap-1 font-medium">
                                      <AlertCircle className="w-3.5 h-3.5" />
                                      {health.error}
                                    </span>
                                  )}
                                </div>
                              )}
                            </div>
                            <div className="flex items-center gap-2">
                              <button
                                onClick={() => handleHealthCheck(conn)}
                                className="px-3 py-1.5 bg-[var(--secondary)] hover:bg-[var(--secondary)]/80 text-[var(--secondary-foreground)] rounded-lg text-xs font-medium flex items-center gap-1 transition"
                              >
                                <Activity className="w-3.5 h-3.5" />
                                {t(locale, 'aiResources.testHealth')}
                              </button>
                            </div>
                          </div>
                        );
                      })
                    )}
                  </div>
                </div>
              )}

              {activeTab === 'credentials' && (
                <div className="space-y-4">
                  <div className="flex justify-between items-center">
                    <h3 className="font-bold text-sm text-[var(--muted-foreground)] uppercase">
                      {t(locale, 'aiResources.credentialsPool')}
                    </h3>
                    <button
                      onClick={() => setShowAddApiKey(true)}
                      className="px-3 py-1.5 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-xs font-semibold flex items-center gap-1.5"
                    >
                      <Plus className="w-3.5 h-3.5" />
                      {t(locale, 'aiResources.addApiKey')}
                    </button>
                  </div>

                  <div className="space-y-3">
                    {credentials.length === 0 ? (
                      <div className="p-8 text-center text-sm text-[var(--muted-foreground)] border border-dashed border-[var(--border)] rounded-xl">
                        {t(locale, 'aiResources.noCredentials')}
                      </div>
                    ) : (
                      credentials.map((cred) => (
                        <div
                          key={cred.id}
                          className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] flex items-center justify-between"
                        >
                          <div className="space-y-1">
                            <div className="font-semibold text-sm flex items-center gap-2">
                              <Key className="w-4 h-4 text-[var(--primary)]" />
                              {cred.label}
                              <span className="text-[10px] uppercase font-bold px-2 py-0.5 rounded bg-[var(--secondary)] text-[var(--secondary-foreground)]">
                                {cred.kind}
                              </span>
                              <span
                                className={`text-[10px] uppercase font-bold px-2 py-0.5 rounded ${
                                  cred.status === 'active'
                                    ? 'bg-[var(--success)]/10 text-[var(--success)] border border-[var(--success)]/20'
                                    : 'bg-[var(--warning)]/10 text-[var(--warning)] border border-[var(--warning)]/20'
                                }`}
                              >
                                {cred.status}
                              </span>
                            </div>
                            <div className="text-xs text-[var(--muted-foreground)] font-mono flex items-center gap-3">
                              <span>Identity: {cred.maskedIdentity}</span>
                              <span>Priority: {cred.priority}</span>
                              <span>Concurrency: {cred.concurrencyLimit}</span>
                            </div>
                          </div>
                          <div className="flex items-center gap-2">
                            {cred.kind === 'oauth' && (
                              <button
                                onClick={async () => {
                                  try {
                                    await aiApi.oauthRefresh(cred.id);
                                    await loadProviderDetails(selectedProvider.id);
                                  } catch (e) {
                                    console.error(e);
                                  }
                                }}
                                className="p-2 text-[var(--primary)] hover:bg-[var(--primary)]/10 rounded-lg transition"
                                title="Refresh Token"
                              >
                                <RefreshCw className="w-4 h-4" />
                              </button>
                            )}
                            <button
                              onClick={() => handleDeleteCredential(cred.id)}
                              className="p-2 text-[var(--destructive)] hover:bg-[var(--destructive)]/10 rounded-lg transition"
                            >
                              <Trash2 className="w-4 h-4" />
                            </button>
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              )}

              {activeTab === 'models' && (
                <div className="space-y-4">
                  <div className="flex justify-between items-center">
                    <h3 className="font-bold text-sm text-[var(--muted-foreground)] uppercase">
                      {t(locale, 'aiResources.modelCatalog')} ({models.length})
                    </h3>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => {
                          setShowDiscoverModels(true);
                        }}
                        className="px-3 py-1.5 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-xs font-semibold flex items-center gap-1.5"
                      >
                        <Sparkles className="w-3.5 h-3.5" />
                        {t(locale, 'aiResources.discoverModels')}
                      </button>
                      <button
                        onClick={() => setShowManualModel(true)}
                        className="px-3 py-1.5 bg-[var(--secondary)] text-[var(--secondary-foreground)] rounded-lg text-xs font-semibold flex items-center gap-1.5"
                      >
                        <Plus className="w-3.5 h-3.5" />
                        {t(locale, 'aiResources.addManual')}
                      </button>
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-3">
                    {models.length === 0 ? (
                      <div className="col-span-2 p-8 text-center text-sm text-[var(--muted-foreground)] border border-dashed border-[var(--border)] rounded-xl">
                        {t(locale, 'aiResources.noModels')}
                      </div>
                    ) : (
                      models.map((m) => (
                        <div
                          key={m.id}
                          className="p-3.5 rounded-xl bg-[var(--card)] border border-[var(--border)] flex items-center justify-between"
                        >
                          <div className="space-y-1 min-w-0">
                            <div className="font-semibold text-sm truncate">{m.displayName || m.modelId}</div>
                            <div className="text-xs text-[var(--muted-foreground)] font-mono truncate">{m.modelId}</div>
                            <div className="flex items-center gap-2 text-[10px]">
                              <span className="px-1.5 py-0.5 rounded bg-[var(--secondary)] text-[var(--secondary-foreground)] uppercase">
                                {m.source}
                              </span>
                              <span
                                className={`px-1.5 py-0.5 rounded ${
                                  m.availability === 'available'
                                    ? 'text-[var(--success)] bg-[var(--success)]/10'
                                    : 'text-[var(--muted-foreground)] bg-[var(--muted)]'
                                }`}
                              >
                                {m.availability}
                              </span>
                            </div>
                          </div>
                          <button
                            onClick={async () => {
                              await aiApi.deleteModel(m.id);
                              await loadProviderDetails(selectedProvider.id);
                            }}
                            className="p-1.5 text-[var(--muted-foreground)] hover:text-[var(--destructive)] rounded transition"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              )}

              {activeTab === 'quota' && (
                <div className="space-y-4">
                  <h3 className="font-bold text-sm text-[var(--muted-foreground)] uppercase">
                    {t(locale, 'aiResources.quotaWindows')}
                  </h3>

                  {credentials.filter((c) => c.kind === 'oauth').length === 0 ? (
                    <div className="p-8 text-center text-sm text-[var(--muted-foreground)] border border-dashed border-[var(--border)] rounded-xl">
                      {t(locale, 'aiResources.noOauthAccounts')}
                    </div>
                  ) : (
                    credentials
                      .filter((c) => c.kind === 'oauth')
                      .map((cred) => {
                        const q = quotas[cred.id];
                        return (
                          <div key={cred.id} className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] space-y-3">
                            <div className="flex items-center justify-between">
                              <div className="font-bold text-sm flex items-center gap-2">
                                {cred.label}
                                <span className="text-xs text-[var(--muted-foreground)] font-normal">({cred.maskedIdentity})</span>
                              </div>
                              <span className="text-xs px-2 py-0.5 rounded bg-[var(--success)]/10 text-[var(--success)] border border-[var(--success)]/20 font-semibold">
                                {q?.status || 'Active'}
                              </span>
                            </div>

                            {q?.windows && q.windows.length > 0 ? (
                              <div className="grid grid-cols-2 gap-3 mt-2">
                                {q.windows.map((w) => (
                                  <div key={w.id} className="p-3 rounded-lg bg-[var(--secondary)]/40 border border-[var(--border)]">
                                    <div className="text-xs text-[var(--muted-foreground)] font-semibold">{w.label}</div>
                                    <div className="text-lg font-bold mt-1 text-[var(--primary)]">
                                      {w.remaining ?? '100'} {w.unit || '%'}
                                    </div>
                                    <div className="text-[10px] text-[var(--muted-foreground)] mt-0.5">
                                      Limit: {w.limitValue ?? 'Unlimited'}
                                    </div>
                                  </div>
                                ))}
                              </div>
                            ) : (
                              <div className="text-xs text-[var(--muted-foreground)] py-2">
                                {t(locale, 'aiResources.noQuotaConstraints')}
                              </div>
                            )}
                          </div>
                        );
                      })
                  )}
                </div>
              )}
            </div>
          ) : (
            <div className="flex-1 flex items-center justify-center p-12 text-center text-[var(--muted-foreground)]">
              {t(locale, 'aiResources.selectProviderHint')}
            </div>
          )}
        </div>
      </div>

      {showAddProvider && (
        <AddProviderModal
          onClose={() => setShowAddProvider(false)}
          onSuccess={(p) => {
            void loadSummaryAndProviders();
            setSelectedProvider(p);
          }}
        />
      )}

      {showAddConnection && selectedProvider && (
        <AddConnectionModal
          providerId={selectedProvider.id}
          defaultName={`${selectedProvider.name} Endpoint`}
          defaultBaseUrl={selectedProvider.websiteUrl || 'https://api.openai.com/v1'}
          onClose={() => setShowAddConnection(false)}
          onSuccess={() => {
            void loadProviderDetails(selectedProvider.id);
            void loadSummaryAndProviders();
          }}
        />
      )}

      {showAddApiKey && selectedProvider && (
        <AddApiKeyModal
          providerId={selectedProvider.id}
          connectionIds={connections.map((c) => c.id)}
          onClose={() => setShowAddApiKey(false)}
          onSuccess={() => {
            void loadProviderDetails(selectedProvider.id);
            void loadSummaryAndProviders();
          }}
        />
      )}

      {showOAuthModal && (
        <OAuthConnectModal
          onClose={() => setShowOAuthModal(false)}
          onSuccess={() => {
            void loadSummaryAndProviders();
            if (selectedProvider) {
              void loadProviderDetails(selectedProvider.id);
            }
          }}
        />
      )}

      {showDiscoverModels && selectedProvider && (
        <DiscoverModelsModal
          providerId={selectedProvider.id}
          connectionId={connections[0]?.id}
          defaultBaseUrl={connections[0]?.baseUrl || selectedProvider.websiteUrl || ''}
          onClose={() => setShowDiscoverModels(false)}
          onSuccess={() => {
            void loadProviderDetails(selectedProvider.id);
            void loadSummaryAndProviders();
          }}
        />
      )}

      {showManualModel && selectedProvider && (
        <ManualModelModal
          providerId={selectedProvider.id}
          connectionId={connections[0]?.id}
          onClose={() => setShowManualModel(false)}
          onSuccess={() => {
            void loadProviderDetails(selectedProvider.id);
            void loadSummaryAndProviders();
          }}
        />
      )}

      {deleteImpact && (
        <DeleteImpactModal
          impact={deleteImpact}
          onConfirm={confirmDeleteProvider}
          onCancel={() => setDeleteImpact(null)}
        />
      )}
    </div>
  );
}
