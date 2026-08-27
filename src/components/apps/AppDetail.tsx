'use client';

import React, { useState, useEffect, useCallback } from 'react';
import {
  Laptop,
  Globe,
  Layers,
  Activity,
  Monitor,
  Copy,
  Check,
} from 'lucide-react';
import { t, useLocale } from '@/i18n';
import {
  appsApi,
  type AppView,
  type RuntimeInstance,
  type Surface,
  type SystemApplicationSpec,
  type WebApplicationSpec,
} from '@/lib/tauri/apps';
import { AppActionBar } from './AppActionBar';

interface AppDetailProps {
  app: AppView | null;
  loadingAction: boolean;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
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
  onEdit,
  onRemove,
  onToggleSidebar,
  onClearData,
}: AppDetailProps) {
  const locale = useLocale();
  const [instances, setInstances] = useState<RuntimeInstance[]>([]);
  const [surfaces, setSurfaces] = useState<Surface[]>([]);
  const [webSpec, setWebSpec] = useState<WebApplicationSpec | null>(null);
  const [systemSpec, setSystemSpec] = useState<SystemApplicationSpec | null>(null);
  const [copied, setCopied] = useState(false);
  const [_loadingDetails, setLoadingDetails] = useState(false);

  const loadDetails = useCallback(async (targetApp: AppView) => {
    setLoadingDetails(true);
    setWebSpec(null);
    setSystemSpec(null);
    try {
      const [insts, surfs, wSpec, sSpec] = await Promise.all([
        appsApi.listInstances(targetApp.appId),
        appsApi.listSurfaces(targetApp.appId),
        targetApp.kind === 'web_application' ? appsApi.getWebSpec(targetApp.appId) : Promise.resolve(null),
        targetApp.kind === 'system_application' ? appsApi.getSystemSpec(targetApp.appId) : Promise.resolve(null),
      ]);
      setInstances(insts);
      setSurfaces(surfs);
      setWebSpec(wSpec);
      setSystemSpec(sSpec);
    } catch (err) {
      console.warn('Failed to load app details:', err);
    } finally {
      setLoadingDetails(false);
    }
  }, []);

  const handleCopyUrl = useCallback((urlToCopy: string) => {
    if (!urlToCopy) return;
    void navigator.clipboard.writeText(urlToCopy);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, []);

  useEffect(() => {
    if (app) {
      void loadDetails(app);
    }
  }, [app, loadDetails]);

  if (!app) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center p-12 text-center select-none bg-[var(--surface-subtle)]">
        <Layers className="h-12 w-12 text-[var(--text-tertiary)] stroke-1 mb-3" />
        <p className="text-sm font-medium text-[var(--text-secondary)]">
          {t(locale, 'appsPage.selectApp')}
        </p>
      </div>
    );
  }

  const getKindLabel = (kind: string) => {
    switch (kind) {
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
      case 'system_application':
        return <Laptop className="h-5 w-5 text-[var(--accent)]" />;
      case 'web_application':
        return <Globe className="h-5 w-5 text-[var(--primary)]" />;
      default:
        return <Layers className="h-5 w-5 text-[var(--primary)]" />;
    }
  };

  return (
    <div className="flex-1 flex flex-col h-full overflow-y-auto bg-[var(--surface)]">
      {/* Header Banner */}
      <div className="p-6 border-b border-[var(--border-subtle)] bg-[var(--surface-hover)]/40 space-y-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl bg-[var(--surface-hover)] border border-[var(--border-subtle)] shadow-sm">
              {getKindIcon(app.kind)}
            </div>
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <h1 className="text-lg font-bold text-[var(--text)]">
                  {app.title}
                </h1>
                <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-[var(--surface-hover)] text-[var(--text-secondary)] border border-[var(--border-subtle)]">
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
            onEdit={onEdit}
            onRemove={onRemove}
            onToggleSidebar={onToggleSidebar}
            onClearData={onClearData}
          />
        </div>
      </div>

      {/* Main Details Body */}
      <div className="p-6 space-y-6 flex-1">
        {/* Web Configuration */}
        {app.kind === 'web_application' && (
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
              <Globe className="h-4 w-4 text-[var(--primary)]" />
              <span>{t(locale, 'appsPage.webConfigTitle')}</span>
            </div>

            <div className="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 space-y-4 text-xs shadow-sm">
              {/* Target URL */}
              <div className="space-y-1.5">
                <span className="text-[11px] font-medium text-[var(--text-secondary)]">
                  {t(locale, 'appsPage.webTargetUrl')}
                </span>
                {webSpec?.url ? (
                  <div className="flex items-center gap-2 p-2.5 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)]">
                    <span className="font-mono text-xs text-[var(--primary)] select-all truncate flex-1">
                      {webSpec.url}
                    </span>
                    <button
                      type="button"
                      onClick={() => handleCopyUrl(webSpec.url)}
                      className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-[var(--text-secondary)] hover:text-[var(--text)] hover:bg-[var(--surface)] transition-colors border border-[var(--border-subtle)] shrink-0"
                      title={t(locale, 'appsPage.copyUrl')}
                    >
                      {copied ? (
                        <>
                          <Check className="h-3.5 w-3.5 text-[var(--success)]" />
                          <span>{t(locale, 'appsPage.copied')}</span>
                        </>
                      ) : (
                        <>
                          <Copy className="h-3.5 w-3.5" />
                          <span>{t(locale, 'appsPage.copyUrl')}</span>
                        </>
                      )}
                    </button>
                  </div>
                ) : (
                  <div className="p-2.5 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-[var(--text-tertiary)]">
                    —
                  </div>
                )}
              </div>
              <p className="text-[11px] text-[var(--text-tertiary)]">
                {t(locale, 'appsPage.webLinkHint')}
              </p>
            </div>
          </div>
        )}

        {/* System Configuration */}
        {app.kind === 'system_application' && systemSpec && (
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
              <Laptop className="h-4 w-4 text-[var(--accent)]" />
              <span>{t(locale, 'appsPage.systemConfigTitle')}</span>
            </div>

            <div className="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 space-y-3 text-xs shadow-sm">
              <div className="space-y-1">
                <span className="text-[11px] font-medium text-[var(--text-secondary)]">
                  {t(locale, 'appsPage.applicationPath')}
                </span>
                <p className="font-mono text-xs text-[var(--text)] p-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] truncate select-all">
                  {systemSpec.applicationPath || '—'}
                </p>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1 border-t border-[var(--border-subtle)]">
                <div className="space-y-1">
                  <span className="text-[11px] font-medium text-[var(--text-secondary)]">
                    {t(locale, 'appsPage.bundleIdLabel')}
                  </span>
                  <p className="font-mono text-[11px] text-[var(--text-secondary)]">
                    {systemSpec.bundleIdentifier || '—'}
                  </p>
                </div>
                <div className="space-y-1">
                  <span className="text-[11px] font-medium text-[var(--text-secondary)]">
                    {t(locale, 'appsPage.launchPolicy')}
                  </span>
                  <div>
                    <span className="px-2 py-0.5 rounded text-[10px] font-medium bg-[var(--surface-hover)] text-[var(--text-secondary)] border border-[var(--border-subtle)]">
                      {systemSpec.launchPolicy || 'default'}
                    </span>
                  </div>
                </div>
              </div>
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
              <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-hover)] p-4 text-xs text-[var(--text-tertiary)]">
                {t(locale, 'appsPage.noInstances')}
              </div>
            ) : (
              <div className="space-y-2">
                {instances.map((inst) => (
                  <div
                    key={inst.id}
                    className="flex items-center justify-between p-3 rounded-xl border border-[var(--border)] bg-[var(--surface)] text-xs"
                  >
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="font-mono font-medium text-[var(--text)]">
                          PID: {inst.pid || '—'}
                        </span>
                        {inst.pgid && (
                          <span className="text-[10px] text-[var(--text-tertiary)]">
                            PGID: {inst.pgid}
                          </span>
                        )}
                        {inst.ownershipMode && (
                          <span className="px-1.5 py-0.5 rounded text-[10px] bg-[var(--surface-hover)] text-[var(--text-secondary)]">
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
                          : 'bg-[var(--surface-hover)] text-[var(--text-tertiary)]'
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
        {app.kind !== 'web_application' && <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
            <Monitor className="h-4 w-4" />
            <span>{t(locale, 'appsPage.surfaces')}</span>
          </div>

          {surfaces.length === 0 ? (
            <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-hover)] p-4 text-xs text-[var(--text-tertiary)]">
              {t(locale, 'appsPage.noSurfaces')}
            </div>
          ) : (
            <div className="space-y-2">
              {surfaces.map((surf) => (
                <div
                  key={surf.id}
                  className="flex items-center justify-between p-3 rounded-xl border border-[var(--border)] bg-[var(--surface)] text-xs"
                >
                  <div className="space-y-0.5">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-[var(--text)]">
                        {surf.label || surf.kind}
                      </span>
                      <span className="text-[10px] text-[var(--text-tertiary)]">
                        ({surf.kind})
                      </span>
                    </div>
                    {surf.url && (
                      <p className="text-[11px] text-[var(--primary)] truncate max-w-md">
                        {surf.url}
                      </p>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>}
      </div>
    </div>
  );
}
