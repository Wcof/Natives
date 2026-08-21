'use client';

import { useState, useEffect, useCallback } from 'react';
import { aiApi, type Provider, type Connection, type Credential, type Model } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { ShieldCheck, Plus, Trash2, Activity, Key, Cpu, RefreshCw, CheckCircle2, AlertCircle } from 'lucide-react';

export default function AiResourcesPanel() {
  const locale = useLocale();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<Provider | null>(null);
  const [connections, setConnections] = useState<Connection[]>([]);
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(false);
  const [healthStatus, setHealthStatus] = useState<Record<string, { reachable: boolean; error: string | null }>>({});

  // Add Key Modal State
  const [showAddKey, setShowAddKey] = useState(false);
  const [keyLabel, setKeyLabel] = useState('');
  const [keyValue, setKeyValue] = useState('');
  const [keyError, setKeyError] = useState<string | null>(null);

  const loadProviders = useCallback(async () => {
    setLoading(true);
    try {
      const list = await aiApi.listProviders();
      setProviders(list);
      if (list.length > 0 && !selectedProvider && list[0]) {
        setSelectedProvider(list[0]);
      }
    } catch (e) {
      console.error('Failed to load providers:', e);
    } finally {
      setLoading(false);
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
        setModels([]);
      }
    } catch (e) {
      console.error('Failed to load provider details:', e);
    }
  }, []);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

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
      setHealthStatus((prev) => ({
        ...prev,
        [conn.id]: { reachable: false, error: String(e) },
      }));
    }
  };

  const handleAddCredential = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedProvider || !keyValue.trim()) return;
    setKeyError(null);
    try {
      await aiApi.createCredential({
        providerId: selectedProvider.id,
        label: keyLabel.trim() || 'Default Key',
        secret: keyValue.trim(),
      });
      setShowAddKey(false);
      setKeyLabel('');
      setKeyValue('');
      await loadProviderDetails(selectedProvider.id);
    } catch (err) {
      setKeyError(String(err));
    }
  };

  const handleDeleteCredential = async (credId: string) => {
    if (!selectedProvider) return;
    try {
      await aiApi.deleteCredential({
        providerId: selectedProvider.id,
        credentialId: credId,
      });
      await loadProviderDetails(selectedProvider.id);
    } catch (err) {
      console.error('Failed to delete credential:', err);
    }
  };

  return (
    <div className="flex flex-col gap-6 w-full max-w-5xl mx-auto p-4">
      {/* Header Info */}
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] pb-4">
        <div>
          <h2 className="text-lg font-semibold text-[var(--text)] flex items-center gap-2">
            <Cpu className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'aiResources.title')}
          </h2>
          <p className="text-xs text-[var(--text-secondary)] mt-1">
            {t(locale, 'aiResources.desc')}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadProviders()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs bg-[var(--surface-hover)] text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          {t(locale, 'aiResources.refresh')}
        </button>
      </div>

      {/* Main Grid: Left Provider List, Right Details */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
        {/* Provider List */}
        <div className="flex flex-col gap-2">
          <span className="text-xs font-semibold text-[var(--text-secondary)] uppercase tracking-wider">
            {t(locale, 'aiResources.providers')}
          </span>
          <div className="flex flex-col gap-1.5">
            {providers.map((p) => {
              const isSelected = selectedProvider?.id === p.id;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setSelectedProvider(p)}
                  className={`flex flex-col items-start p-3 rounded-lg border text-left transition-all ${
                    isSelected
                      ? 'border-[var(--primary)] bg-[var(--primary-subtle)] text-[var(--text)]'
                      : 'border-[var(--border-subtle)] bg-[var(--surface)] text-[var(--text-secondary)] hover:border-[var(--border)]'
                  }`}
                >
                  <div className="flex items-center justify-between w-full">
                    <span className="font-medium text-sm text-[var(--text)]">{p.name}</span>
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--surface-hover)] text-[var(--text-muted)]">
                      {p.apiProtocol}
                    </span>
                  </div>
                  <span className="text-xs text-[var(--text-muted)] truncate w-full mt-1">
                    {p.baseUrl || 'Default Base URL'}
                  </span>
                </button>
              );
            })}
            {providers.length === 0 && (
              <div className="p-4 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-lg">
                {t(locale, 'aiResources.noProviders')}
              </div>
            )}
          </div>
        </div>

        {/* Selected Provider Details */}
        <div className="md:col-span-2 flex flex-col gap-6">
          {selectedProvider ? (
            <>
              {/* Connections Section */}
              <div className="flex flex-col gap-3 p-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)]">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-[var(--text)] flex items-center gap-2">
                    <Activity className="w-4 h-4 text-[var(--primary)]" />
                    {t(locale, 'aiResources.connections')}
                  </h3>
                </div>
                <div className="flex flex-col gap-2">
                  {connections.map((conn) => {
                    const health = healthStatus[conn.id];
                    return (
                      <div
                        key={conn.id}
                        className="flex items-center justify-between p-3 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)]"
                      >
                        <div className="flex flex-col gap-0.5">
                          <span className="text-xs font-semibold text-[var(--text)]">{conn.name}</span>
                          <span className="text-[11px] font-mono text-[var(--text-secondary)]">{conn.baseUrl}</span>
                          <span className="text-[10px] text-[var(--text-muted)]">Protocol: {conn.apiProtocol}</span>
                        </div>
                        <div className="flex items-center gap-2">
                          {health && (
                            <span
                              className={`flex items-center gap-1 text-[11px] px-2 py-0.5 rounded ${
                                health.reachable
                                  ? 'text-[var(--success)] bg-[var(--success-soft)]'
                                  : 'text-[var(--danger)] bg-[var(--danger-soft)]'
                              }`}
                            >
                              {health.reachable ? <CheckCircle2 className="w-3.5 h-3.5" /> : <AlertCircle className="w-3.5 h-3.5" />}
                              {health.reachable ? t(locale, 'aiResources.reachable') : t(locale, 'aiResources.unreachable')}
                            </span>
                          )}
                          <button
                            type="button"
                            onClick={() => void handleHealthCheck(conn)}
                            className="px-2.5 py-1 text-xs rounded border border-[var(--border-subtle)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-colors"
                          >
                            {t(locale, 'aiResources.probe')}
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* Credentials (Multi-Key) Section */}
              <div className="flex flex-col gap-3 p-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)]">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="text-sm font-semibold text-[var(--text)] flex items-center gap-2">
                      <Key className="w-4 h-4 text-[var(--primary)]" />
                      {t(locale, 'aiResources.credentials')}
                    </h3>
                    <span className="text-[11px] text-[var(--text-muted)] flex items-center gap-1 mt-0.5">
                      <ShieldCheck className="w-3.5 h-3.5 text-[var(--success)]" />
                      {t(locale, 'aiResources.keychainNotice')}
                    </span>
                  </div>
                  <button
                    type="button"
                    onClick={() => setShowAddKey(true)}
                    className="flex items-center gap-1 px-2.5 py-1 text-xs rounded bg-[var(--primary)] text-[var(--primary-foreground)] font-medium hover:opacity-90 transition-opacity"
                  >
                    <Plus className="w-3.5 h-3.5" />
                    {t(locale, 'aiResources.addKey')}
                  </button>
                </div>

                <div className="flex flex-col gap-2">
                  {credentials.map((c) => (
                    <div
                      key={c.id}
                      className="flex items-center justify-between p-3 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)]"
                    >
                      <div className="flex flex-col gap-0.5">
                        <div className="flex items-center gap-2">
                          <span className="text-xs font-semibold text-[var(--text)]">{c.label}</span>
                          <span className="text-[10px] px-1.5 py-0.2 rounded bg-[var(--success-soft)] text-[var(--success)]">
                            Keychain Ref
                          </span>
                        </div>
                        <span className="text-[11px] font-mono text-[var(--text-muted)]">{c.maskedKey}</span>
                        <span className="text-[10px] text-[var(--text-muted)] font-mono">{c.secretRef}</span>
                      </div>
                      <button
                        type="button"
                        onClick={() => void handleDeleteCredential(c.id)}
                        className="p-1.5 rounded text-[var(--text-muted)] hover:text-[var(--danger)] hover:bg-[var(--danger-soft)] transition-colors"
                        title={t(locale, 'aiResources.deleteKey')}
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>
                  ))}
                  {credentials.length === 0 && (
                    <div className="p-4 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-lg">
                      {t(locale, 'aiResources.noKeys')}
                    </div>
                  )}
                </div>
              </div>

              {/* Models Catalog Section */}
              <div className="flex flex-col gap-3 p-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)]">
                <h3 className="text-sm font-semibold text-[var(--text)] flex items-center gap-2">
                  <Cpu className="w-4 h-4 text-[var(--primary)]" />
                  {t(locale, 'aiResources.models')}
                </h3>
                <div className="flex flex-wrap gap-2">
                  {models.map((m) => (
                    <div
                      key={m.id}
                      className="flex flex-col px-3 py-1.5 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs"
                    >
                      <span className="font-semibold text-[var(--text)]">{m.displayName || m.modelId}</span>
                      <span className="text-[10px] font-mono text-[var(--text-muted)]">{m.modelId}</span>
                    </div>
                  ))}
                  {models.length === 0 && (
                    <span className="text-xs text-[var(--text-muted)]">{t(locale, 'aiResources.noModels')}</span>
                  )}
                </div>
              </div>
            </>
          ) : (
            <div className="p-12 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-xl">
              {t(locale, 'aiResources.selectProvider')}
            </div>
          )}
        </div>
      </div>

      {/* Add Key Modal */}
      {showAddKey && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-[color-mix(in_srgb,var(--neutral-0)_50%,transparent)] backdrop-blur-sm p-4">
          <div className="w-full max-w-md p-6 rounded-2xl bg-[var(--surface)] border border-[var(--border)] shadow-xl flex flex-col gap-4">
            <h3 className="text-base font-semibold text-[var(--text)]">{t(locale, 'aiResources.addKeyTitle')}</h3>
            <p className="text-xs text-[var(--text-secondary)]">
              {t(locale, 'aiResources.addKeyDesc')}
            </p>
            <form onSubmit={(e) => void handleAddCredential(e)} className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'aiResources.label')}</label>
                <input
                  type="text"
                  value={keyLabel}
                  onChange={(e) => setKeyLabel(e.target.value)}
                  placeholder={t(locale, 'aiResources.labelPlaceholder')}
                  className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none focus:border-[var(--primary)]"
                />
              </div>
              <div className="flex flex-col gap-1">
                <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'aiResources.secret')}</label>
                <input
                  type="password"
                  value={keyValue}
                  onChange={(e) => setKeyValue(e.target.value)}
                  placeholder="sk-..."
                  required
                  className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] font-mono focus:outline-none focus:border-[var(--primary)]"
                />
              </div>
              {keyError && <span className="text-xs text-[var(--danger)]">{keyError}</span>}
              <div className="flex items-center justify-end gap-2 mt-2">
                <button
                  type="button"
                  onClick={() => setShowAddKey(false)}
                  className="px-3 py-1.5 rounded-lg text-xs border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                >
                  {t(locale, 'aiResources.cancel')}
                </button>
                <button
                  type="submit"
                  className="px-4 py-1.5 rounded-lg text-xs bg-[var(--primary)] text-[var(--primary-foreground)] font-medium hover:opacity-90"
                >
                  {t(locale, 'aiResources.saveToKeychain')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
