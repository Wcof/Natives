'use client';

import React from 'react';
import { Folder, Github, Package, Plus, RefreshCw } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';
import CreativeHome from '@/components/creative/CreativeHome';
import CreativeDock, { type CreativeDockTab } from '@/components/creative/CreativeDock';
import { classifyError } from '@/lib/error-classifier';
import type { AddMenu } from '@/hooks/useCreativeImport';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface DockController {
  tabs: CreativeDockTab[];
  activeKey: string | null;
  select: (tab: CreativeDockTab) => void;
  minimize: (tab: CreativeDockTab) => void;
  close: (tab: CreativeDockTab) => void;
}

export interface CatalogShellProps {
  locale: Locale;
  loading: boolean;
  error: unknown;
  apps: CreativeAppSummary[];
  busyIds: ReadonlySet<string>;
  onReload: () => void;
  onOpenApp: (app: CreativeAppSummary) => void;
  onStartApp: (app: CreativeAppSummary) => void;
  onStopApp: (app: CreativeAppSummary) => void;
  onDeleteApp: (app: CreativeAppSummary) => void;
  onRestartApp?: (app: CreativeAppSummary) => void;
  onAppLogs?: (app: CreativeAppSummary) => void;
  onRunSettings?: (app: CreativeAppSummary) => void;
  onResolveOrphan?: (app: CreativeAppSummary, restart: boolean) => void;
  onInstallDeps?: (app: CreativeAppSummary) => void;
  addMenu: AddMenu;
  setAddMenu: (menu: AddMenu) => void;
  onAddImport: () => void;
  onAddLocal: () => void;
  onAddGithub: () => void;
  /** Proposal inbox controller output, rendered above the catalog. */
  inbox: React.ReactNode;
  dock: DockController;
}

/**
 * The workshop catalog shell: header (refresh + add menu), proposal inbox,
 * the creative home/catalog, and the running-app dock. Pure wiring — business
 * rules stay in the controllers/hooks.
 */
export default function CatalogShell({
  locale,
  loading,
  error,
  apps,
  busyIds,
  onReload,
  onOpenApp,
  onStartApp,
  onStopApp,
  onDeleteApp,
  onRestartApp,
  onAppLogs,
  onRunSettings,
  onResolveOrphan,
  onInstallDeps,
  addMenu,
  setAddMenu,
  onAddImport,
  onAddLocal,
  onAddGithub,
  inbox,
  dock,
}: CatalogShellProps) {
  return (
    <>
      <div className="px-6 py-4 border-b border-[var(--border)] flex items-center justify-between shrink-0 bg-[var(--surface)]">
        <div>
          <h2 className="text-base font-bold text-[var(--text)] tracking-tight">
            {t(locale, 'workshop.title')}
          </h2>
          <p className="text-xs text-[var(--text-secondary)] mt-0.5">
            {t(locale, 'workshop.subtitle')}
          </p>
        </div>
        <div className="flex items-center gap-2 relative">
          <button
            type="button"
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all"
            onClick={onReload}
            title={t(locale, 'common.refresh')}
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
          </button>

          <button
            type="button"
            className="flex h-8 items-center gap-1.5 px-3 rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] text-xs font-medium hover:opacity-90 active:scale-95 transition-all shadow-sm"
            onClick={() => setAddMenu(addMenu === 'open' ? 'closed' : 'open')}
          >
            <Plus size={14} />
            <span>{t(locale, 'workshop.add')}</span>
          </button>

          {addMenu === 'open' && (
            <>
              <div className="fixed inset-0 z-10" onClick={() => setAddMenu('closed')} />
              <div className="absolute right-0 top-full mt-1.5 w-52 bg-[var(--surface)] border border-[var(--border)] rounded-xl shadow-lg p-1 z-20 flex flex-col gap-0.5">
                {/* The only creation entry is the one-sentence composer in the
                    creative home; import flows start here. */}
                <button
                  type="button"
                  className="flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all w-full text-left"
                  onClick={onAddImport}
                >
                  <Package size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuImport')}</span>
                </button>
                <button
                  type="button"
                  className="flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all w-full text-left"
                  onClick={onAddLocal}
                >
                  <Folder size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuLocal')}</span>
                </button>
                <button
                  type="button"
                  className="flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all w-full text-left"
                  onClick={onAddGithub}
                >
                  <Github size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuGithub')}</span>
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      <div className="p-6 flex-1">
        {loading && <LoadingState />}
        {Boolean(error) && (
          <EmptyState
            title={t(locale, 'common.error')}
            description={typeof error === 'string' ? error : classifyError(error).userMessage}
            action={{ label: t(locale, 'common.retry'), onClick: onReload }}
          />
        )}
        {inbox}
        <CreativeHome
          locale={locale}
          apps={apps}
          busyIds={busyIds}
          onReloadApps={onReload}
          onImport={() => setAddMenu('open')}
          onOpenApp={onOpenApp}
          onStartApp={onStartApp}
          onStopApp={onStopApp}
          onDeleteApp={onDeleteApp}
          onRestartApp={onRestartApp}
          onAppLogs={onAppLogs}
          onRunSettings={onRunSettings}
          onResolveOrphan={onResolveOrphan}
          onInstallDeps={onInstallDeps}
        />
      </div>

      <CreativeDock
        tabs={dock.tabs}
        activeKey={dock.activeKey}
        onSelect={dock.select}
        onMinimize={dock.minimize}
        onClose={dock.close}
      />
    </>
  );
}
