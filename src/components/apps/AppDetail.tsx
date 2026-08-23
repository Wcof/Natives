'use client';

import React, { useState, useEffect, useCallback } from 'react';
import {
  Folder,
  Laptop,
  Globe,
  Layers,
  Activity,
  Monitor,
  FileText,
} from 'lucide-react';
import { t, useLocale } from '@/i18n';
import {
  appsApi,
  type AppView,
  type RuntimeInstance,
  type Surface,
} from '@/lib/tauri/apps';
import { AppActionBar } from './AppActionBar';

interface AppDetailProps {
  app: AppView | null;
  loadingAction: boolean;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onForceStop: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onToggleSidebar: () => void;
  onClearData: () => void;
}

export function AppDetail({
  app,
  loadingAction,
  onOpen,
  onStart,
  onStop,
  onRestart,
  onForceStop,
  onEdit,
  onRemove,
  onToggleSidebar,
  onClearData,
}: AppDetailProps) {
  const locale = useLocale();
  const [instances, setInstances] = useState<RuntimeInstance[]>([]);
  const [surfaces, setSurfaces] = useState<Surface[]>([]);
  const [logs, setLogs] = useState<Array<{ seq: number; text: string; stream: string }>>([]);
  const [showLogs, setShowLogs] = useState(false);
  const [_loadingDetails, setLoadingDetails] = useState(false);

  const loadDetails = useCallback(async (targetApp: AppView) => {
    setLoadingDetails(true);
    try {
      const [insts, surfs] = await Promise.all([
        appsApi.listInstances(targetApp.appId),
        appsApi.listSurfaces(targetApp.appId),
      ]);
      setInstances(insts);
      setSurfaces(surfs);
      if (targetApp.kind === 'local_project') {
        const logLines = await appsApi.localLogs(targetApp.appId, 100);
        setLogs(logLines);
      }
    } catch (err) {
      console.warn('Failed to load app details:', err);
    } finally {
      setLoadingDetails(false);
    }
  }, []);

  useEffect(() => {
    if (app) {
      void loadDetails(app);
    }
  }, [app, loadDetails]);

  if (!app) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center p-12 text-center select-none bg-[var(--surface-overlay)]/30">
        <Layers className="h-12 w-12 text-[var(--text-tertiary)] stroke-1 mb-3" />
        <p className="text-sm font-medium text-[var(--text-secondary)]">
          {t(locale, 'appsPage.selectApp')}
        </p>
      </div>
    );
  }

  const getKindLabel = (kind: string) => {
    switch (kind) {
      case 'local_project':
        return t(locale, 'appsPage.sourceLocal');
      case 'system_application':
        return t(locale, 'appsPage.sourceNative');
      case 'web_application':
        return t(locale, 'appsPage.sourceRemote');
      default:
        return kind;
    }
  };

  const getKindIcon = (kind: string) => {
    switch (kind) {
      case 'local_project':
        return <Folder className="h-5 w-5 text-[var(--success)]" />;
      case 'system_application':
        return <Laptop className="h-5 w-5 text-[var(--interactive-accent)]" />;
      case 'web_application':
        return <Globe className="h-5 w-5 text-[var(--primary)]" />;
      default:
        return <Layers className="h-5 w-5 text-[var(--interactive-accent)]" />;
    }
  };

  return (
    <div className="flex-1 flex flex-col h-full overflow-y-auto bg-[var(--surface-base)]">
      {/* Header Banner */}
      <div className="p-6 border-b border-[var(--border-default)] bg-[var(--surface-overlay)]/40 space-y-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-[var(--surface-muted)] border border-[var(--border-subtle)] shadow-sm">
              {getKindIcon(app.kind)}
            </div>
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <h1 className="text-lg font-bold text-[var(--text-primary)]">
                  {app.title}
                </h1>
                <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-[var(--surface-muted)] text-[var(--text-secondary)] border border-[var(--border-subtle)]">
                  {getKindLabel(app.kind)}
                </span>
              </div>
              {app.description && (
                <p className="text-xs text-[var(--text-secondary)] leading-relaxed">
                  {app.description}
                </p>
              )}
            </div>
          </div>

          {/* Action Bar */}
          <AppActionBar
            app={app}
            loading={loadingAction}
            onOpen={onOpen}
            onStart={onStart}
            onStop={onStop}
            onRestart={onRestart}
            onForceStop={onForceStop}
            onEdit={onEdit}
            onRemove={onRemove}
            onToggleSidebar={onToggleSidebar}
            onClearData={onClearData}
            onViewLogs={() => setShowLogs(!showLogs)}
          />
        </div>
      </div>

      {/* Main Details Body */}
      <div className="p-6 space-y-6 flex-1">
        {/* Logs Panel (toggleable for local projects) */}
        {showLogs && app.kind === 'local_project' && (
          <div className="rounded-2xl border border-[var(--border-default)] bg-[var(--surface-base)] p-4 font-mono text-xs text-[var(--success)] space-y-2">
            <div className="flex items-center justify-between pb-2 border-b border-[var(--border-subtle)] text-[var(--text-secondary)] font-sans">
              <span className="flex items-center gap-1.5 font-medium">
                <FileText className="h-4 w-4" />
                {t(locale, 'appsPage.logs')}
              </span>
              <button
                type="button"
                onClick={() => void loadDetails(app)}
                className="hover:text-[var(--text-primary)] transition-colors"
              >
                {t(locale, 'common.refresh')}
              </button>
            </div>
            <div className="max-h-56 overflow-y-auto space-y-0.5">
              {logs.length === 0 ? (
                <p className="text-[var(--text-tertiary)]">No logs captured yet.</p>
              ) : (
                logs.map((l, i) => (
                  <div key={i} className="leading-tight">
                    <span className="text-[var(--text-tertiary)] mr-2">[{l.stream}]</span>
                    <span>{l.text}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {/* Runtime Instances */}
        {app.kind !== 'web_application' && (
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
              <Activity className="h-4 w-4" />
              <span>{t(locale, 'appsPage.runtimeInstances')}</span>
            </div>

            {instances.length === 0 ? (
              <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-muted)] p-4 text-xs text-[var(--text-tertiary)]">
                {t(locale, 'appsPage.noInstances')}
              </div>
            ) : (
              <div className="space-y-2">
                {instances.map((inst) => (
                  <div
                    key={inst.id}
                    className="flex items-center justify-between p-3 rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] text-xs"
                  >
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="font-mono font-medium text-[var(--text-primary)]">
                          PID: {inst.pid || '—'}
                        </span>
                        {inst.pgid && (
                          <span className="text-[10px] text-[var(--text-tertiary)]">
                            PGID: {inst.pgid}
                          </span>
                        )}
                        {inst.ownershipMode && (
                          <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--surface-muted)] text-[var(--text-secondary)]">
                            {inst.ownershipMode}
                          </span>
                        )}
                      </div>
                      {inst.currentPort && (
                        <p className="text-[11px] text-[var(--text-secondary)]">
                          Port: {inst.currentPort}
                        </p>
                      )}
                    </div>
                    <span
                      className={`px-2 py-0.5 rounded-full text-[10px] font-semibold ${
                        inst.status === 'running'
                          ? 'bg-[var(--success-soft)] text-[var(--success)]'
                          : 'bg-[var(--surface-muted)] text-[var(--text-tertiary)]'
                      }`}
                    >
                      {inst.status}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Surfaces & Presentation */}
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
            <Monitor className="h-4 w-4" />
            <span>{t(locale, 'appsPage.surfaces')}</span>
          </div>

          {surfaces.length === 0 ? (
            <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-muted)] p-4 text-xs text-[var(--text-tertiary)]">
              {t(locale, 'appsPage.noSurfaces')}
            </div>
          ) : (
            <div className="space-y-2">
              {surfaces.map((surf) => (
                <div
                  key={surf.id}
                  className="flex items-center justify-between p-3 rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] text-xs"
                >
                  <div className="space-y-0.5">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-[var(--text-primary)]">
                        {surf.label || surf.kind}
                      </span>
                      <span className="text-[10px] text-[var(--text-tertiary)]">
                        ({surf.kind})
                      </span>
                    </div>
                    {surf.url && (
                      <p className="text-[11px] text-[var(--interactive-accent)] truncate max-w-md">
                        {surf.url}
                      </p>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
