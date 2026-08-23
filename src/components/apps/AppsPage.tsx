'use client';

import React, { useState, useEffect, useCallback } from 'react';
import { subscribe } from '@/lib/tauri/core';
import { appsApi, type AppView } from '@/lib/tauri/apps';
import { t, useLocale } from '@/i18n';
import { AppList } from './AppList';
import { AppDetail } from './AppDetail';
import { AddAppDialog } from './AddAppDialog';
import { EditAppDialog } from './EditAppDialog';
import { AppRiskDialog } from './AppRiskDialog';

export default function AppsPage() {
  const locale = useLocale();
  const [apps, setApps] = useState<AppView[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
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
    try {
      const list = await appsApi.listViews();
      setApps(list);
      setSelectedId((prev) => {
        if (prev && list.some((a) => a.appId === prev)) return prev;
        return list.length > 0 ? list[0]?.appId ?? null : null;
      });
    } catch (err) {
      console.error('Failed to load apps:', err);
    } finally {
      setLoading(false);
    }
  }, []);

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
    try {
      await appsApi.open(app.appId);
      await loadApps();
    } catch (err) {
      console.error('Failed to open app:', err);
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
      console.error('Failed to start app:', err);
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
        console.error('Failed to stop app:', err);
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
      console.error('Failed to restart app:', err);
    } finally {
      setOperatingAppId(null);
    }
  };

  const handleForceStop = async (app: AppView) => {
    setRiskAction({
      isOpen: true,
      title: t(locale, 'appsPage.forceStop'),
      description: t(locale, 'appsPage.riskLevel2Desc'),
      riskLevel: 2,
      action: async () => {
        setOperatingAppId(app.appId);
        try {
          await appsApi.forceStop(app.appId);
          await loadApps();
        } catch (err) {
          console.error('Failed to force stop app:', err);
        } finally {
          setOperatingAppId(null);
        }
      },
    });
  };

  const handleRemove = async (app: AppView) => {
    const note =
      app.kind === 'local_project'
        ? t(locale, 'appsPage.riskRemoveLocalNote')
        : app.kind === 'system_application'
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
          console.error('Failed to remove app:', err);
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
          console.error('Failed to clear web data:', err);
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
      console.error('Failed to toggle sidebar visibility:', err);
    }
  };

  return (
    <div className="flex h-full w-full overflow-hidden bg-[var(--surface-base)]">
      {/* App List Sidebar Panel */}
      <AppList
        apps={apps}
        selectedId={selectedId}
        loading={loading}
        onSelect={(app) => setSelectedId(app.appId)}
        onAdd={() => setShowAddDialog(true)}
        onRefresh={() => void loadApps()}
      />

      {/* App Detail Main View */}
      <AppDetail
        app={selectedApp}
        loadingAction={operatingAppId === selectedApp?.appId}
        onOpen={() => selectedApp && void handleOpen(selectedApp)}
        onStart={() => selectedApp && void handleStart(selectedApp)}
        onStop={() => selectedApp && void handleStop(selectedApp)}
        onRestart={() => selectedApp && void handleRestart(selectedApp)}
        onForceStop={() => selectedApp && void handleForceStop(selectedApp)}
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
