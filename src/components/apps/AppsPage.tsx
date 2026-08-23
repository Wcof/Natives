'use client';

import React, { useState, useEffect, useCallback } from 'react';
import { subscribe } from '@/lib/tauri/core';
import { appsApi, type AppView, type SystemRunningState } from '@/lib/tauri/apps';
import { t, useLocale } from '@/i18n';
import { AppList } from './AppList';
import { AppDetail } from './AppDetail';
import { AddAppDialog } from './AddAppDialog';
import { EditAppDialog } from './EditAppDialog';
import { AppRiskDialog } from './AppRiskDialog';
import { classifyError } from '@/lib/error-classifier';

export default function AppsPage({ onOpenApp }: { onOpenApp?: (appId: string) => void }) {
  const locale = useLocale();
  const [apps, setApps] = useState<AppView[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [systemStates, setSystemStates] = useState<Record<string, SystemRunningState>>({});
  const [operatingAppId, setOperatingAppId] = useState<string | null>(null);

  // Dialog States
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [editingApp, setEditingApp] = useState<AppView | null>(null);
  const [riskAction, setRiskAction] = useState<{
    isOpen: boolean;
    title: string;
    description: string;
    note?: string;
    riskLevel: number;
    action: () => Promise<void>;
  }>({
    isOpen: false,
    title: '',
    description: '',
    riskLevel: 1,
    action: async () => {},
  });

  const loadApps = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const list = await appsApi.listViews();
      const productApps = list.filter((app) => app.kind !== 'local_project');
      setApps(productApps);
      const observed = await Promise.all(
        productApps.filter((app) => app.kind === 'system_application').map(async (app) => {
          try { return [app.appId, await appsApi.systemObserve(app.appId)] as const; }
          catch { return null; }
        }),
      );
      setSystemStates(Object.fromEntries(observed.filter((item): item is readonly [string, SystemRunningState] => item !== null)));
      setSelectedId((prev) => {
        if (prev && productApps.some((a) => a.appId === prev)) return prev;
        return productApps[0]?.appId ?? null;
      });
    } catch (err) {
      setLoadError(classifyError(err, { locale }).userMessage);
    } finally {
      setLoading(false);
    }
  }, [locale]);

  useEffect(() => {
    void loadApps();
  }, [loadApps]);

  // ── Event-driven refresh (APP-019 / APP-059) ──────────────────────────
  useEffect(() => {
    const unsubscribe = subscribe<{ channel: string; data?: unknown }>(
      'db-state-changed',
      (payload) => {
        if (payload.channel === 'apps') {
          void loadApps();
        }
      }
    );
    return () => {
      unsubscribe();
    };
  }, [loadApps]);

  const selectedApp = apps.find((a) => a.appId === selectedId) || null;

  // ── Action Handlers ───────────────────────────────────────────────────

  const handleOpen = async (app: AppView) => {
    setOperatingAppId(app.appId);
    setActionError(null);
    try {
      if (onOpenApp) onOpenApp(app.appId);
      else await appsApi.open(app.appId);
    } catch (err) {
      setActionError(classifyError(err, { locale }).userMessage);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleStart = async (app: AppView) => {
    setOperatingAppId(app.appId);
    try {
      await appsApi.start(app.appId);
      await loadApps();
    } catch (err) {
      setActionError(classifyError(err, { locale }).userMessage);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleStop = async (app: AppView) => {
    const isPreexistingSystem =
      app.kind === 'system_application' && app.capabilities.riskLevel >= 2;

    const performStop = async () => {
      setOperatingAppId(app.appId);
      try {
        await appsApi.stop(app.appId);
        await loadApps();
      } catch (err) {
        setActionError(classifyError(err, { locale }).userMessage);
      } finally {
        setOperatingAppId(null);
      }
    };

    if (isPreexistingSystem) {
      setRiskAction({
        isOpen: true,
        title: t(locale, 'appsPage.riskModalTitle'),
        description: t(locale, 'appsPage.riskLevel2Desc'),
        note: t(locale, 'appsPage.riskPreexistingSystemStopNote'),
        riskLevel: 2,
        action: performStop,
      });
    } else {
      await performStop();
    }
  };

  const handleRestart = async (app: AppView) => {
    setOperatingAppId(app.appId);
    try {
      await appsApi.restart(app.appId);
      await loadApps();
    } catch (err) {
      setActionError(classifyError(err, { locale }).userMessage);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleRemove = async (app: AppView) => {
    const note = app.kind === 'system_application'
        ? t(locale, 'appsPage.riskRemoveSystemNote')
        : t(locale, 'appsPage.riskRemoveWebNote');

    setRiskAction({
      isOpen: true,
      title: t(locale, 'appsPage.removeApp'),
      description: t(locale, 'appsPage.riskLevel1Desc'),
      note,
      riskLevel: 1,
      action: async () => {
        setOperatingAppId(app.appId);
        try {
          await appsApi.remove(app.appId, 1);
          await loadApps();
        } catch (err) {
          setActionError(classifyError(err, { locale }).userMessage);
        } finally {
          setOperatingAppId(null);
        }
      },
    });
  };

  const handleClearWebData = async (app: AppView) => {
    setRiskAction({
      isOpen: true,
      title: t(locale, 'appsPage.clearData'),
      description: t(locale, 'appsPage.riskLevel2Desc'),
      note: t(locale, 'appsPage.riskClearWebDataNote'),
      riskLevel: 2,
      action: async () => {
        setOperatingAppId(app.appId);
        try {
          await appsApi.webClearData(app.appId);
          await loadApps();
        } catch (err) {
          setActionError(classifyError(err, { locale }).userMessage);
        } finally {
          setOperatingAppId(null);
        }
      },
    });
  };

  const handleToggleSidebar = async (app: AppView) => {
    try {
      await appsApi.setSidebarVisibility(app.appId, !app.showInSidebar);
      await loadApps();
    } catch (err) {
      setActionError(classifyError(err, { locale }).userMessage);
    }
  };

  return (
    <div className="relative flex h-full w-full overflow-hidden bg-[var(--surface-base)]">
      {/* App List Sidebar Panel */}
      <AppList
        apps={apps}
        systemStates={systemStates}
        selectedId={selectedId}
        loading={loading}
        onSelect={(app) => setSelectedId(app.appId)}
        onAdd={() => setShowAddDialog(true)}
        onRefresh={() => void loadApps()}
      />

      {(loadError || actionError) && (
        <div className="absolute left-1/2 top-4 z-20 -translate-x-1/2 rounded-xl border border-[var(--danger)]/30 bg-[var(--surface-overlay)] px-4 py-2 text-xs text-[var(--danger)] shadow-lg">
          {loadError || actionError}
          {loadError && <button type="button" onClick={() => void loadApps()} className="ml-3 underline">{t(locale, 'common.retry')}</button>}
        </div>
      )}

      {/* App Detail Main View */}
      <AppDetail
        app={selectedApp}
        loadingAction={operatingAppId === selectedApp?.appId}
        onOpen={() => selectedApp && void handleOpen(selectedApp)}
        onStart={() => selectedApp && void handleStart(selectedApp)}
        onStop={() => selectedApp && void handleStop(selectedApp)}
        onRestart={() => selectedApp && void handleRestart(selectedApp)}
        onEdit={() => selectedApp && setEditingApp(selectedApp)}
        onRemove={() => selectedApp && void handleRemove(selectedApp)}
        onToggleSidebar={() => selectedApp && void handleToggleSidebar(selectedApp)}
        onClearData={() => selectedApp && void handleClearWebData(selectedApp)}
      />

      {/* Add Dialog */}
      <AddAppDialog
        isOpen={showAddDialog}
        onSuccess={(newApp) => {
          setShowAddDialog(false);
          void loadApps();
          setSelectedId(newApp.appId);
        }}
        onClose={() => setShowAddDialog(false)}
      />

      {/* Edit Dialog */}
      <EditAppDialog
        isOpen={!!editingApp}
        app={editingApp}
        onSuccess={(updated) => {
          setEditingApp(null);
          void loadApps();
          setSelectedId(updated.appId);
        }}
        onClose={() => setEditingApp(null)}
      />

      {/* Risk Confirmation Dialog */}
      <AppRiskDialog
        isOpen={riskAction.isOpen}
        title={riskAction.title}
        description={riskAction.description}
        note={riskAction.note}
        riskLevel={riskAction.riskLevel}
        onConfirm={async () => {
          await riskAction.action();
          setRiskAction((prev) => ({ ...prev, isOpen: false }));
        }}
        onCancel={() => setRiskAction((prev) => ({ ...prev, isOpen: false }))}
      />
    </div>
  );
}
