'use client';

import { useState, useEffect, useCallback } from 'react';
import {
  proxyApi,
  type ProxyStatusDTO,
  type Route,
  type ProxyUsageRecord,
  type ProxySettings,
} from '@/lib/tauri/proxy';
import { aiApi, type Connection, type Model } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import {
  Play,
  Square,
  RefreshCw,
  Copy,
  Check,
  Server,
  Layers,
  Activity,
  Plus,
  Trash2,
  AlertCircle,
  Clock,
  Globe,
} from 'lucide-react';

export default function LocalProxyPanel() {
  const locale = useLocale();
  const [status, setStatus] = useState<ProxyStatusDTO | null>(null);
  const [settings, setSettings] = useState<ProxySettings | null>(null);
  const [routes, setRoutes] = useState<Route[]>([]);
  const [usageRecords, setUsageRecords] = useState<ProxyUsageRecord[]>([]);
  const [toggling, setToggling] = useState(false);
  const [copiedPath, setCopiedPath] = useState<string | null>(null);

  // Available connections & models for route builder
  const [connections, setConnections] = useState<Connection[]>([]);
  const [models, setModels] = useState<Model[]>([]);

  // Route modal states
  const [showAddRoute, setShowAddRoute] = useState(false);
  const [routeForm, setRouteForm] = useState<{
    id: string;
    localModel: string;
    connectionId: string;
    upstreamModel: string;
    poolPolicy: 'priority_round_robin' | 'round_robin' | 'least_inflight';
  }>({
    id: '',
    localModel: '',
    connectionId: '',
    upstreamModel: '',
    poolPolicy: 'priority_round_robin',
  });

  const loadAll = useCallback(async () => {
    try {
      const [s, cfg, r, u, conns, m] = await Promise.all([
        proxyApi.status(),
        proxyApi.getSettings(),
        proxyApi.listRoutes(),
        proxyApi.listUsageRecords(50, 0),
        aiApi.listConnections(),
        aiApi.listModels(),
      ]);
      setStatus(s);
      setSettings(cfg);
      setRoutes(r);
      setUsageRecords(u);
      setConnections(conns);
      setModels(m);
    } catch (e) {
      console.error('Failed to load proxy state:', e);
    }
  }, []);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  const [bannerError, setBannerError] = useState<string | null>(null);

  const handleStart = async () => {
    setToggling(true);
    setBannerError(null);
    try {
      await proxyApi.start();
      await loadAll();
    } catch (e) {
      setBannerError(String(e));
    } finally {
      setToggling(false);
    }
  };

  const handleStop = async () => {
    setToggling(true);
    setBannerError(null);
    try {
      await proxyApi.stop();
      await loadAll();
    } catch (e) {
      setBannerError(String(e));
    } finally {
      setToggling(false);
    }
  };

  const handleRestart = async () => {
    setToggling(true);
    setBannerError(null);
    try {
      await proxyApi.restart();
      await loadAll();
    } catch (e) {
      setBannerError(String(e));
    } finally {
      setToggling(false);
    }
  };

  const copyEndpoint = (url: string, key: string) => {
    navigator.clipboard.writeText(url);
    setCopiedPath(key);
    setTimeout(() => setCopiedPath(null), 2000);
  };

  const handleSaveRoute = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!routeForm.localModel.trim() || !routeForm.connectionId || !routeForm.upstreamModel.trim()) {
      return;
    }

    const routeObj: Route = {
      id: routeForm.id || `route-${Date.now()}`,
      localModel: routeForm.localModel.trim(),
      enabled: true,
      strategy: 'ordered',
      targets: [
        {
          id: `target-${Date.now()}`,
          routeId: routeForm.id || `route-${Date.now()}`,
          position: 0,
          connectionId: routeForm.connectionId,
          modelId: routeForm.upstreamModel.trim(),
          credentialSelector: { pool: { policy: routeForm.poolPolicy } },
          priority: 0,
          enabled: true,
        },
      ],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    try {
      await proxyApi.saveRoute(routeObj);
      setShowAddRoute(false);
      setRouteForm({
        id: '',
        localModel: '',
        connectionId: '',
        upstreamModel: '',
        poolPolicy: 'priority_round_robin',
      });
      await loadAll();
    } catch (err) {
      setBannerError(String(err));
    }
  };

  const handleDeleteRoute = async (id: string) => {
    try {
      await proxyApi.deleteRoute(id);
      await loadAll();
    } catch (err) {
      setBannerError(String(err));
    }
  };

  return (
    <div className="flex flex-col h-full bg-[var(--background)] text-[var(--foreground)] overflow-y-auto">
      {/* Top Header */}
      <div className="p-6 border-b border-[var(--border)] bg-[var(--card)]/40 backdrop-blur-md">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold flex items-center gap-2 font-display text-[var(--foreground)]">
              <Server className="w-7 h-7 text-[var(--primary)]" />
              {t(locale, 'localProxy.title')}
            </h1>
            <p className="text-sm text-[var(--muted-foreground)] mt-1">
              {t(locale, 'localProxy.desc')}
            </p>
          </div>
          <div className="flex items-center gap-3">
            <button
              onClick={handleRestart}
              disabled={toggling || !status?.running}
              className="px-3.5 py-2 bg-[var(--secondary)] hover:bg-[var(--secondary)]/80 text-[var(--secondary-foreground)] rounded-lg font-medium text-sm flex items-center gap-2 transition disabled:opacity-50"
            >
              <RefreshCw className={`w-4 h-4 ${toggling ? 'animate-spin' : ''}`} />
              {t(locale, 'localProxy.restart')}
            </button>
            {status?.running ? (
              <button
                onClick={handleStop}
                disabled={toggling}
                className="px-4 py-2 bg-[var(--destructive)] hover:bg-[var(--destructive)]/90 text-[var(--primary-foreground)] rounded-lg font-medium text-sm flex items-center gap-2 shadow-sm transition disabled:opacity-50"
              >
                <Square className="w-4 h-4 fill-current" />
                {t(locale, 'localProxy.stopProxy')}
              </button>
            ) : (
              <button
                onClick={handleStart}
                disabled={toggling}
                className="px-4 py-2 bg-[var(--primary)] hover:bg-[var(--primary)]/90 text-[var(--primary-foreground)] rounded-lg font-medium text-sm flex items-center gap-2 shadow-sm transition disabled:opacity-50"
              >
                <Play className="w-4 h-4 fill-current" />
                {t(locale, 'localProxy.startProxy')}
              </button>
            )}
          </div>
        </div>
      </div>

      <div className="p-6 space-y-6 max-w-7xl mx-auto w-full">
        {bannerError && (
          <div className="p-4 rounded-xl bg-[var(--destructive)]/10 border border-[var(--destructive)]/20 text-[var(--destructive)] text-sm flex items-center justify-between">
            <div className="flex items-center gap-2">
              <AlertCircle className="w-5 h-5 shrink-0" />
              <span>{bannerError}</span>
            </div>
            <button onClick={() => setBannerError(null)} className="p-1 rounded text-xs font-semibold hover:underline">
              ✕
            </button>
          </div>
        )}

        {/* Runtime Status & Endpoints Grid */}
        <div className="grid grid-cols-2 gap-6">
          {/* Card 1: Runtime Status */}
          <div className="p-5 rounded-2xl bg-[var(--card)] border border-[var(--border)] shadow-sm space-y-4">
            <div className="flex items-center justify-between">
              <h2 className="font-bold text-base flex items-center gap-2">
                <Activity className="w-5 h-5 text-[var(--primary)]" />
                {t(locale, 'localProxy.runtimeStatus')}
              </h2>
              <span
                className={`px-2.5 py-1 rounded-full text-xs font-semibold uppercase flex items-center gap-1.5 ${
                  status?.running
                    ? 'bg-[var(--success)]/10 text-[var(--success)] border border-[var(--success)]/20'
                    : 'bg-[var(--muted)] text-[var(--muted-foreground)] border border-[var(--border)]'
                }`}
              >
                <span className={`w-2 h-2 rounded-full ${status?.running ? 'bg-[var(--success)] animate-pulse' : 'bg-[var(--muted-foreground)]'}`} />
                {status?.status || 'Stopped'}
              </span>
            </div>

            <div className="grid grid-cols-2 gap-3 pt-2">
              <div className="p-3 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)]">
                <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'localProxy.engine')}</div>
                <div className="text-sm font-bold mt-0.5">{status?.engine || 'host-native'}</div>
              </div>
              <div className="p-3 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)]">
                <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'localProxy.effectivePort')}</div>
                <div className="text-sm font-mono font-bold mt-0.5">{status?.effectivePort || settings?.configuredPort || 15721}</div>
              </div>
              <div className="p-3 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)]">
                <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'localProxy.bindHost')}</div>
                <div className="text-sm font-mono font-bold mt-0.5">{status?.host || '127.0.0.1'}</div>
              </div>
              <div className="p-3 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)]">
                <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'localProxy.activeRequests')}</div>
                <div className="text-sm font-bold mt-0.5 text-[var(--primary)]">{status?.activeRequests || 0}</div>
              </div>
            </div>

            {status?.lastError && (
              <div className="p-3 rounded-xl bg-[var(--destructive)]/10 border border-[var(--destructive)]/20 text-[var(--destructive)] text-xs flex items-center gap-2">
                <AlertCircle className="w-4 h-4 shrink-0" />
                <span>{status.lastError}</span>
              </div>
            )}
          </div>

          {/* Card 2: Local Compatible Endpoints */}
          <div className="p-5 rounded-2xl bg-[var(--card)] border border-[var(--border)] shadow-sm space-y-4">
            <h2 className="font-bold text-base flex items-center gap-2">
              <Globe className="w-5 h-5 text-[var(--primary)]" />
              {t(locale, 'localProxy.compatibleEndpoints')}
            </h2>

            <div className="space-y-2.5">
              {[
                { name: 'OpenAI Chat Completions', path: '/v1/chat/completions' },
                { name: 'OpenAI Responses', path: '/v1/responses' },
                { name: 'Anthropic Messages', path: '/v1/messages' },
                { name: 'Models Catalog', path: '/v1/models' },
              ].map((ep) => {
                const port = status?.effectivePort || settings?.configuredPort || 15721;
                const fullUrl = `http://127.0.0.1:${port}${ep.path}`;
                return (
                  <div
                    key={ep.path}
                    className="p-2.5 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)] flex items-center justify-between"
                  >
                    <div className="min-w-0 pr-2">
                      <div className="text-xs font-semibold text-[var(--muted-foreground)]">{ep.name}</div>
                      <div className="text-xs font-mono truncate text-[var(--foreground)] mt-0.5">{fullUrl}</div>
                    </div>
                    <button
                      onClick={() => copyEndpoint(fullUrl, ep.path)}
                      className="p-1.5 rounded-lg hover:bg-[var(--secondary)] text-[var(--muted-foreground)] hover:text-[var(--foreground)] transition shrink-0"
                      title="Copy URL"
                    >
                      {copiedPath === ep.path ? <Check className="w-4 h-4 text-[var(--success)]" /> : <Copy className="w-4 h-4" />}
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        {/* Section 2: Model Routes */}
        <div className="space-y-4">
          <div className="flex justify-between items-center">
            <div>
              <h2 className="font-bold text-lg flex items-center gap-2">
                <Layers className="w-5 h-5 text-[var(--primary)]" />
                {t(locale, 'localProxy.modelRoutes')}
              </h2>
              <p className="text-xs text-[var(--muted-foreground)] mt-0.5">
                {t(locale, 'localProxy.routesDesc')}
              </p>
            </div>
            <button
              onClick={() => {
                if (connections.length === 0) {
                  setBannerError(t(locale, 'localProxy.noRoutes'));
                  return;
                }
                setRouteForm({
                  id: '',
                  localModel: '',
                  connectionId: connections[0]?.id || '',
                  upstreamModel: models[0]?.modelId || '',
                  poolPolicy: 'priority_round_robin',
                });
                setShowAddRoute(true);
              }}
              className="px-3.5 py-1.5 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-xs font-semibold flex items-center gap-1.5 shadow-sm"
            >
              <Plus className="w-4 h-4" />
              {t(locale, 'localProxy.addRoute')}
            </button>
          </div>

          <div className="grid grid-cols-2 gap-4">
            {routes.length === 0 ? (
              <div className="col-span-2 p-12 text-center text-sm text-[var(--muted-foreground)] border border-dashed border-[var(--border)] rounded-2xl">
                {t(locale, 'localProxy.noRoutes')}
              </div>
            ) : (
              routes.map((route) => {
                const target = route.targets[0];
                const conn = connections.find((c) => c.id === target?.connectionId);
                return (
                  <div
                    key={route.id}
                    className="p-4 rounded-2xl bg-[var(--card)] border border-[var(--border)] shadow-sm space-y-3"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-bold font-mono text-[var(--primary)]">{route.localModel}</span>
                        <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--success)]/10 text-[var(--success)] border border-[var(--success)]/20 uppercase font-semibold">
                          Active
                        </span>
                      </div>
                      <button
                        onClick={() => handleDeleteRoute(route.id)}
                        className="p-1.5 text-[var(--muted-foreground)] hover:text-[var(--destructive)] rounded transition"
                      >
                        <Trash2 className="w-4 h-4" />
                      </button>
                    </div>

                    {target && (
                      <div className="p-3 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)] text-xs space-y-1.5">
                        <div className="flex items-center justify-between text-[var(--muted-foreground)]">
                          <span>Upstream Connection:</span>
                          <span className="font-semibold text-[var(--foreground)]">{conn?.name || target.connectionId}</span>
                        </div>
                        <div className="flex items-center justify-between text-[var(--muted-foreground)]">
                          <span>Upstream Model:</span>
                          <span className="font-mono font-semibold text-[var(--foreground)]">{target.modelId}</span>
                        </div>
                        <div className="flex items-center justify-between text-[var(--muted-foreground)]">
                          <span>Pool Policy:</span>
                          <span className="capitalize font-semibold text-[var(--foreground)]">
                            {'pool' in target.credentialSelector ? target.credentialSelector.pool.policy : 'Direct'}
                          </span>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>

        {/* Section 3: Recent Usage Audit */}
        <div className="space-y-4 pt-2">
          <h2 className="font-bold text-lg flex items-center gap-2">
            <Clock className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'localProxy.recentUsage')} ({usageRecords.length})
          </h2>

          <div className="rounded-2xl border border-[var(--border)] bg-[var(--card)] overflow-hidden shadow-sm">
            <table className="w-full text-xs text-left">
              <thead className="bg-[var(--secondary)]/50 border-b border-[var(--border)] text-[var(--muted-foreground)] font-semibold uppercase text-[10px]">
                <tr>
                  <th className="p-3">Model</th>
                  <th className="p-3">Protocols (In → Up)</th>
                  <th className="p-3">Tokens (Prompt / Comp)</th>
                  <th className="p-3">Latency</th>
                  <th className="p-3">Status</th>
                  <th className="p-3">Time</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border)]">
                {usageRecords.length === 0 ? (
                  <tr>
                    <td colSpan={6} className="p-6 text-center text-[var(--muted-foreground)]">
                      {t(locale, 'localProxy.noUsage')}
                    </td>
                  </tr>
                ) : (
                  usageRecords.slice(0, 15).map((u) => (
                    <tr key={u.id} className="hover:bg-[var(--secondary)]/20 transition">
                      <td className="p-3 font-mono font-medium">{u.localModel}</td>
                      <td className="p-3 text-[var(--muted-foreground)]">
                        <span className="font-mono">{u.inboundProtocol}</span> → <span className="font-mono">{u.upstreamProtocol}</span>
                      </td>
                      <td className="p-3 font-mono">
                        {u.promptTokens} / {u.completionTokens} ({u.totalTokens})
                      </td>
                      <td className="p-3 font-mono">{u.latencyMs}ms</td>
                      <td className="p-3">
                        <span
                          className={`px-2 py-0.5 rounded text-[10px] uppercase font-semibold ${
                            u.status === 'success' ? 'text-[var(--success)] bg-[var(--success)]/10' : 'text-[var(--destructive)] bg-[var(--destructive)]/10'
                          }`}
                        >
                          {u.status}
                        </span>
                      </td>
                      <td className="p-3 text-[var(--muted-foreground)]">{new Date(u.createdAt).toLocaleTimeString()}</td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      {/* Modal: Add Route */}
      {showAddRoute && (
        <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-md p-6 shadow-2xl space-y-4">
            <div className="flex items-center justify-between">
              <h3 className="font-bold text-lg">{t(locale, 'localProxy.addRouteModal')}</h3>
              <button onClick={() => setShowAddRoute(false)} className="p-1 rounded text-[var(--muted-foreground)] hover:text-[var(--foreground)]">
                ✕
              </button>
            </div>
            <form onSubmit={handleSaveRoute} className="space-y-3">
              <div>
                <label className="text-xs font-semibold text-[var(--muted-foreground)]">{t(locale, 'localProxy.localModelAlias')}</label>
                <input
                  type="text"
                  required
                  placeholder="e.g. gpt-4o, coding, claude-3-7"
                  value={routeForm.localModel}
                  onChange={(e) => setRouteForm((f) => ({ ...f, localModel: e.target.value }))}
                  className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm font-mono focus:outline-none focus:border-[var(--primary)]"
                />
              </div>

              <div>
                <label className="text-xs font-semibold text-[var(--muted-foreground)]">{t(locale, 'localProxy.targetConnection')}</label>
                <select
                  value={routeForm.connectionId}
                  onChange={(e) => setRouteForm((f) => ({ ...f, connectionId: e.target.value }))}
                  className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
                >
                  {connections.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} ({c.upstreamProtocol})
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="text-xs font-semibold text-[var(--muted-foreground)]">{t(locale, 'localProxy.upstreamModelId')}</label>
                <input
                  type="text"
                  required
                  placeholder="e.g. gpt-4o-2024-08-06, claude-3-7-sonnet-20250219"
                  value={routeForm.upstreamModel}
                  onChange={(e) => setRouteForm((f) => ({ ...f, upstreamModel: e.target.value }))}
                  className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm font-mono focus:outline-none focus:border-[var(--primary)]"
                />
              </div>

              <div>
                <label className="text-xs font-semibold text-[var(--muted-foreground)]">{t(locale, 'localProxy.poolPolicy')}</label>
                <select
                  value={routeForm.poolPolicy}
                  onChange={(e) =>
                    setRouteForm((f) => ({
                      ...f,
                      poolPolicy: e.target.value as 'priority_round_robin' | 'round_robin' | 'least_inflight',
                    }))
                  }
                  className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
                >
                  <option value="priority_round_robin">Priority + Round Robin</option>
                  <option value="round_robin">Pure Round Robin</option>
                  <option value="least_inflight">Least In-Flight Calls</option>
                </select>
              </div>

              <div className="flex justify-end gap-2 pt-3">
                <button
                  type="button"
                  onClick={() => setShowAddRoute(false)}
                  className="px-4 py-2 bg-[var(--secondary)] text-[var(--secondary-foreground)] rounded-lg text-sm font-medium"
                >
                  {t(locale, 'localProxy.cancel')}
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-sm font-medium shadow-sm"
                >
                  {t(locale, 'localProxy.saveRoute')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
