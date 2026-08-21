'use client';

import { useState, useEffect, useCallback } from 'react';
import { appsApi, type App, type RuntimeInstance, type Surface } from '@/lib/tauri/apps';
import { useLocale, t } from '@/i18n';
import {
  Layers,
  Play,
  Square,
  RotateCw,
  Plus,
  Trash2,
  ExternalLink,
  Activity,
  RefreshCw,
  CheckCircle2,
  AlertCircle,
  Clock,
} from 'lucide-react';

export default function AppsPage() {
  const locale = useLocale();
  const [apps, setApps] = useState<App[]>([]);
  const [selectedApp, setSelectedApp] = useState<App | null>(null);
  const [instances, setInstances] = useState<RuntimeInstance[]>([]);
  const [surfaces, setSurfaces] = useState<Surface[]>([]);
  const [loading, setLoading] = useState(false);
  const [operatingAppId, setOperatingAppId] = useState<string | null>(null);

  // New App Modal
  const [showAddModal, setShowAddModal] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [newSource, setNewSource] = useState<'local' | 'remote' | 'native'>('local');
  const [newSourceId, setNewSourceId] = useState('');
  const [newDesc, setNewDesc] = useState('');

  const loadApps = useCallback(async () => {
    setLoading(true);
    try {
      const list = await appsApi.list();
      setApps(list);
      if (list.length > 0 && !selectedApp && list[0]) {
        setSelectedApp(list[0]);
      }
    } catch (err) {
      console.error('Failed to load apps:', err);
    } finally {
      setLoading(false);
    }
  }, [selectedApp]);

  const loadAppDetails = useCallback(async (appId: string) => {
    try {
      const [insts, surfs] = await Promise.all([
        appsApi.listInstances(appId),
        appsApi.listSurfaces(appId),
      ]);
      setInstances(insts);
      setSurfaces(surfs);
    } catch (err) {
      console.error('Failed to load app details:', err);
    }
  }, []);

  useEffect(() => {
    void loadApps();
  }, [loadApps]);

  useEffect(() => {
    if (selectedApp) {
      void loadAppDetails(selectedApp.id);
    }
  }, [selectedApp, loadAppDetails]);

  const handleStart = async (appId: string) => {
    setOperatingAppId(appId);
    try {
      await appsApi.start(appId);
      if (selectedApp?.id === appId) {
        await loadAppDetails(appId);
      }
    } catch (err) {
      console.error('Failed to start app:', err);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleStop = async (appId: string) => {
    setOperatingAppId(appId);
    try {
      await appsApi.stop(appId);
      if (selectedApp?.id === appId) {
        await loadAppDetails(appId);
      }
    } catch (err) {
      console.error('Failed to stop app:', err);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleRestart = async (appId: string) => {
    setOperatingAppId(appId);
    try {
      await appsApi.restart(appId);
      if (selectedApp?.id === appId) {
        await loadAppDetails(appId);
      }
    } catch (err) {
      console.error('Failed to restart app:', err);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleDelete = async (appId: string) => {
    try {
      await appsApi.delete(appId);
      if (selectedApp?.id === appId) {
        setSelectedApp(null);
      }
      await loadApps();
    } catch (err) {
      console.error('Failed to delete app:', err);
    }
  };

  const handleCreateApp = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newTitle.trim() || !newSourceId.trim()) return;
    try {
      const created = await appsApi.create({
        title: newTitle.trim(),
        source: newSource,
        sourceId: newSourceId.trim(),
        description: newDesc.trim() || undefined,
      });
      setShowAddModal(false);
      setNewTitle('');
      setNewSourceId('');
      setNewDesc('');
      setSelectedApp(created);
      await loadApps();
    } catch (err) {
      console.error('Failed to create app:', err);
    }
  };

  return (
    <div className="flex flex-col gap-6 w-full max-w-6xl mx-auto p-6 h-full overflow-y-auto">
      {/* Top Header */}
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] pb-4">
        <div>
          <h2 className="text-xl font-semibold text-[var(--text)] flex items-center gap-2">
            <Layers className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'appsPage.title')}
          </h2>
          <p className="text-xs text-[var(--text-secondary)] mt-1">
            {t(locale, 'appsPage.desc')}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void loadApps()}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs bg-[var(--surface-hover)] text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
            {t(locale, 'appsPage.refresh')}
          </button>
          <button
            type="button"
            onClick={() => setShowAddModal(true)}
            className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg text-xs bg-[var(--primary)] text-[var(--primary-foreground)] font-medium hover:opacity-90 transition-opacity"
          >
            <Plus className="w-4 h-4" />
            {t(locale, 'appsPage.addApp')}
          </button>
        </div>
      </div>

      {/* Grid Layout: Left App List, Right Runtime & Surface Details */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
        {/* Left App List */}
        <div className="flex flex-col gap-3">
          <span className="text-xs font-semibold text-[var(--text-secondary)] uppercase tracking-wider">
            {t(locale, 'appsPage.registeredApps')} ({apps.length})
          </span>
          <div className="flex flex-col gap-2">
            {apps.map((app) => {
              const isSelected = selectedApp?.id === app.id;
              return (
                <button
                  key={app.id}
                  type="button"
                  onClick={() => setSelectedApp(app)}
                  className={`flex flex-col p-4 rounded-xl border cursor-pointer text-left transition-all ${
                    isSelected
                      ? 'border-[var(--primary)] bg-[var(--primary-subtle)] text-[var(--text)]'
                      : 'border-[var(--border-subtle)] bg-[var(--surface)] text-[var(--text-secondary)] hover:border-[var(--border)]'
                  }`}
                >
                  <div className="flex items-center justify-between">
                    <span className="font-semibold text-sm text-[var(--text)] truncate max-w-[180px]">
                      {app.title}
                    </span>
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--surface-hover)] uppercase font-mono">
                      {app.source}
                    </span>
                  </div>
                  {app.description && (
                    <span className="text-xs text-[var(--text-muted)] line-clamp-1 mt-1">
                      {app.description}
                    </span>
                  )}
                  <div className="flex items-center justify-between mt-3 text-[11px] text-[var(--text-muted)]">
                    <span className="font-mono">v{app.version}</span>
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        void handleDelete(app.id);
                      }}
                      className="p-1 rounded hover:text-[var(--danger)] transition-colors"
                      title={t(locale, 'appsPage.deleteApp')}
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </button>
              );
            })}
            {apps.length === 0 && (
              <div className="p-8 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-xl">
                {t(locale, 'appsPage.noApps')}
              </div>
            )}
          </div>
        </div>

        {/* Right Details Panel */}
        <div className="md:col-span-2 flex flex-col gap-6">
          {selectedApp ? (
            <>
              {/* App Overview Card */}
              <div className="p-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] flex flex-col gap-4">
                <div className="flex items-center justify-between">
                  <div className="flex flex-col">
                    <h3 className="text-base font-semibold text-[var(--text)]">{selectedApp.title}</h3>
                    <span className="text-xs text-[var(--text-muted)] font-mono mt-0.5">
                      Source ID: {selectedApp.sourceId}
                    </span>
                  </div>
                  {/* Action Buttons */}
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => void handleStart(selectedApp.id)}
                      disabled={operatingAppId === selectedApp.id}
                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs bg-[var(--success-soft)] text-[var(--success)] font-medium hover:bg-[var(--success-soft)] transition-colors"
                    >
                      <Play className="w-3.5 h-3.5 fill-current" />
                      {t(locale, 'appsPage.start')}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleRestart(selectedApp.id)}
                      disabled={operatingAppId === selectedApp.id}
                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs bg-[var(--warning-soft)] text-[var(--warning)] font-medium hover:bg-[var(--warning-soft)] transition-colors"
                    >
                      <RotateCw className="w-3.5 h-3.5" />
                      {t(locale, 'appsPage.restart')}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleStop(selectedApp.id)}
                      disabled={operatingAppId === selectedApp.id}
                      className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs bg-[var(--danger-soft)] text-[var(--danger)] font-medium hover:bg-[var(--danger-soft)] transition-colors"
                    >
                      <Square className="w-3.5 h-3.5 fill-current" />
                      {t(locale, 'appsPage.stop')}
                    </button>
                  </div>
                </div>

                {selectedApp.description && (
                  <p className="text-xs text-[var(--text-secondary)]">{selectedApp.description}</p>
                )}
              </div>

              {/* Runtime Instances Section */}
              <div className="p-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] flex flex-col gap-4">
                <h4 className="text-sm font-semibold text-[var(--text)] flex items-center gap-2">
                  <Activity className="w-4 h-4 text-[var(--primary)]" />
                  {t(locale, 'appsPage.runtimeInstances')}
                </h4>
                <div className="flex flex-col gap-2">
                  {instances.map((inst) => (
                    <div
                      key={inst.id}
                      className="flex items-center justify-between p-3 rounded-xl bg-[var(--surface-hover)] border border-[var(--border-subtle)]"
                    >
                      <div className="flex flex-col gap-1">
                        <div className="flex items-center gap-2">
                          <span
                            className={`flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded-full ${
                              inst.status === 'running'
                                ? 'bg-[var(--success-soft)] text-[var(--success)]'
                                : 'bg-[var(--surface-hover)] text-[var(--text-disabled)]'
                            }`}
                          >
                            {inst.status === 'running' ? (
                              <CheckCircle2 className="w-3.5 h-3.5" />
                            ) : (
                              <AlertCircle className="w-3.5 h-3.5" />
                            )}
                            {inst.status}
                          </span>
                          {inst.pid && (
                            <span className="text-xs font-mono text-[var(--text-secondary)]">PID: {inst.pid}</span>
                          )}
                          {inst.currentPort && (
                            <span className="text-xs font-mono text-[var(--text-secondary)]">
                              Port: {inst.currentPort}
                            </span>
                          )}
                        </div>
                        <span className="text-[10px] text-[var(--text-muted)] font-mono">
                          Instance ID: {inst.id}
                        </span>
                      </div>
                      <span className="text-[10px] text-[var(--text-muted)] flex items-center gap-1">
                        <Clock className="w-3 h-3" />
                        {new Date(inst.updatedAt).toLocaleTimeString()}
                      </span>
                    </div>
                  ))}
                  {instances.length === 0 && (
                    <div className="p-6 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-xl">
                      {t(locale, 'appsPage.noInstances')}
                    </div>
                  )}
                </div>
              </div>

              {/* Surfaces (WebView / Presentation) Section */}
              <div className="p-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)] flex flex-col gap-4">
                <h4 className="text-sm font-semibold text-[var(--text)] flex items-center gap-2">
                  <ExternalLink className="w-4 h-4 text-[var(--primary)]" />
                  {t(locale, 'appsPage.surfaces')}
                </h4>
                <div className="flex flex-col gap-2">
                  {surfaces.map((surf) => (
                    <div
                      key={surf.id}
                      className="flex items-center justify-between p-3 rounded-xl bg-[var(--surface-hover)] border border-[var(--border-subtle)]"
                    >
                      <div className="flex flex-col">
                        <span className="text-xs font-semibold text-[var(--text)]">
                          {surf.label} ({surf.kind})
                        </span>
                        {surf.url && (
                          <span className="text-[11px] font-mono text-[var(--text-secondary)] mt-0.5">
                            {surf.url}
                          </span>
                        )}
                      </div>
                      {surf.url && (
                        <a
                          href={surf.url}
                          target="_blank"
                          rel="noreferrer"
                          className="flex items-center gap-1 px-3 py-1 text-xs rounded-lg bg-[var(--surface)] border border-[var(--border-subtle)] text-[var(--text)] hover:bg-[var(--surface-hover)]"
                        >
                          <ExternalLink className="w-3.5 h-3.5" />
                          {t(locale, 'appsPage.open')}
                        </a>
                      )}
                    </div>
                  ))}
                  {surfaces.length === 0 && (
                    <div className="p-6 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-xl">
                      {t(locale, 'appsPage.noSurfaces')}
                    </div>
                  )}
                </div>
              </div>
            </>
          ) : (
            <div className="p-12 text-center text-xs text-[var(--text-muted)] border border-dashed rounded-2xl">
              {t(locale, 'appsPage.selectApp')}
            </div>
          )}
        </div>
      </div>

      {/* Add App Modal */}
      {showAddModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-[color-mix(in_srgb,var(--neutral-0)_50%,transparent)] backdrop-blur-sm p-4">
          <div className="w-full max-w-md p-6 rounded-2xl bg-[var(--surface)] border border-[var(--border)] shadow-xl flex flex-col gap-4">
            <h3 className="text-base font-semibold text-[var(--text)]">{t(locale, 'appsPage.modalTitle')}</h3>
            <form onSubmit={(e) => void handleCreateApp(e)} className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'appsPage.appTitle')}</label>
                <input
                  type="text"
                  value={newTitle}
                  onChange={(e) => setNewTitle(e.target.value)}
                  placeholder={t(locale, 'appsPage.titlePlaceholder')}
                  required
                  className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none focus:border-[var(--primary)]"
                />
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'appsPage.sourceType')}</label>
                <select
                  value={newSource}
                  onChange={(e) => setNewSource(e.target.value as typeof newSource)}
                  className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none"
                >
                  <option value="local">{t(locale, 'appsPage.sourceLocal')}</option>
                  <option value="remote">{t(locale, 'appsPage.sourceRemote')}</option>
                  <option value="native">{t(locale, 'appsPage.sourceNative')}</option>
                </select>
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-xs text-[var(--text-secondary)]">
                  {newSource === 'local' ? t(locale, 'appsPage.localPathLabel') : newSource === 'remote' ? 'Web URL' : t(locale, 'appsPage.nativePathLabel')}
                </label>
                <input
                  type="text"
                  value={newSourceId}
                  onChange={(e) => setNewSourceId(e.target.value)}
                  placeholder={newSource === 'local' ? '~/projects/my-app' : newSource === 'remote' ? 'https://example.com' : 'open /Applications/...'}
                  required
                  className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] font-mono focus:outline-none focus:border-[var(--primary)]"
                />
              </div>

              <div className="flex flex-col gap-1">
                <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'appsPage.descOptional')}</label>
                <input
                  type="text"
                  value={newDesc}
                  onChange={(e) => setNewDesc(e.target.value)}
                  placeholder={t(locale, 'appsPage.descPlaceholder')}
                  className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none"
                />
              </div>

              <div className="flex items-center justify-end gap-2 mt-2">
                <button
                  type="button"
                  onClick={() => setShowAddModal(false)}
                  className="px-3 py-1.5 rounded-lg text-xs border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                >
                  {t(locale, 'appsPage.cancel')}
                </button>
                <button
                  type="submit"
                  className="px-4 py-1.5 rounded-lg text-xs bg-[var(--primary)] text-[var(--primary-foreground)] font-medium hover:opacity-90"
                >
                  {t(locale, 'appsPage.confirmAdd')}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}
