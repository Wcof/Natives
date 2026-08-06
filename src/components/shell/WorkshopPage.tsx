'use client';

import { useCallback, useRef, useState } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { shouldAutoOpenAfterStart } from '@/lib/creative-app';
import { useCreativeAppCatalog } from '@/hooks/useCreativeAppCatalog';
import { useCreativeWindows } from '@/hooks/useCreativeWindows';
import { useBrowserWindow } from '@/hooks/useBrowserWindow';
import { useCreativeImport } from '@/hooks/useCreativeImport';
import { useCreativeDock } from '@/hooks/useCreativeDock';
import { useModuleImportWizard } from '@/hooks/useModuleImportWizard';
import CatalogShell from './workshop/CatalogShell';
import WindowSurface from './workshop/WindowSurface';
import ModuleImportDialog from './workshop/ModuleImportDialog';
import DeleteDialog from './workshop/DeleteDialog';
import LogsController, { type LogsControllerHandle } from './workshop/LogsController';
import LocalEditDialog, { type LocalEditDialogHandle } from './workshop/LocalEditDialog';
import ProposalInboxController from './workshop/ProposalInboxController';
import LocalImportWizard from './workshop/LocalImportWizard';
import GitHubInstallWizard from './workshop/GitHubInstallWizard';
import DepInstallDialog from './workshop/DepInstallDialog';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

/** WorkshopPage composes the catalog surface and the wizards; each concern
 * lives in its own controller module (T10 convergence). */
export default function WorkshopPage() {
  const prefersReducedMotion = useReducedMotion();
  const locale = useLocale();
  const { apps, loading, error, reload, busyIds, withBusy } = useCreativeAppCatalog();
  const [toast, setToast] = useState<string | null>(null);
  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 2400);
  }, []);

  const browserHostRef = useRef<HTMLDivElement | null>(null);
  const windows = useCreativeWindows(browserHostRef);
  useBrowserWindow(windows.browserApp, browserHostRef);

  const imports = useCreativeImport();
  const dock = useCreativeDock(apps, showToast);
  const moduleImport = useModuleImportWizard(reload, showToast);

  const logsRef = useRef<LogsControllerHandle | null>(null);
  const editRef = useRef<LocalEditDialogHandle | null>(null);
  const [localWizardOpen, setLocalWizardOpen] = useState(false);
  const [githubWizardOpen, setGithubWizardOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<CreativeAppSummary | null>(null);

  // ── Catalog lifecycle handlers ───────────────────────────────────────
  const openInternalModule = useCallback((id: string) => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: `module:${id}` }));
  }, []);

  const handleOpen = useCallback(
    async (app: CreativeAppSummary) => {
      if (app.source === 'internal') await openInternalModule(app.id);
      else await windows.openExternal(app, showToast);
    },
    [openInternalModule, showToast, windows],
  );

  const handleStart = useCallback(
    async (app: CreativeAppSummary) => {
      if (busyIds.has(app.id)) return;
      await withBusy(app.id, async () => {
        try {
          const result = await window.nativesAPI?.creativeApp?.start?.(app.id);
          const updated = result?.summary;
          if (
            updated?.state === 'running' &&
            shouldAutoOpenAfterStart(updated) &&
            updated.source !== 'internal'
          ) {
            await windows.openExternal(updated, showToast);
          }
        } catch (err) {
          showToast(classifyError(err).userMessage);
        }
      });
    },
    [busyIds, showToast, windows, withBusy],
  );

  const handleStop = useCallback(
    async (app: CreativeAppSummary) => {
      if (busyIds.has(app.id)) return;
      if (windows.browserApp?.id === app.id) await windows.closeBrowser();
      await withBusy(app.id, async () => {
        try {
          await window.nativesAPI?.creativeApp?.stop?.(app.id);
        } catch (err) {
          showToast(classifyError(err).userMessage);
        }
      });
    },
    [busyIds, showToast, windows, withBusy],
  );

  const handleRestart = useCallback(
    async (app: CreativeAppSummary) => {
      if (busyIds.has(app.id)) return;
      await withBusy(app.id, async () => {
        try {
          await window.nativesAPI?.creativeApp?.restart?.(app.id);
          void reload();
        } catch (err) {
          showToast(classifyError(err).userMessage);
        }
      });
    },
    [busyIds, reload, showToast, withBusy],
  );

  const handleResolveOrphan = useCallback(
    async (app: CreativeAppSummary, restart: boolean) => {
      if (busyIds.has(app.id)) return;
      await withBusy(app.id, async () => {
        try {
          await window.nativesAPI?.creativeApp?.resolveOrphan?.(app.id, restart);
          void reload();
        } catch (err) {
          showToast(classifyError(err).userMessage);
        }
      });
    },
    [busyIds, reload, showToast, withBusy],
  );

  const handleAddImport = useCallback(() => {
    void imports.pickAndImport(moduleImport.beginImport, showToast);
  }, [imports, moduleImport.beginImport, showToast]);

  if (windows.browserApp) {
    return (
      <WindowSurface
        app={windows.browserApp}
        url={windows.browserUrl}
        hostRef={browserHostRef}
        onStop={handleStop}
        onRestart={handleRestart}
        onClose={() => void windows.closeBrowser()}
        onToast={showToast}
      />
    );
  }

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={
        prefersReducedMotion ? undefined : { type: 'spring', stiffness: 60, damping: 16, mass: 1 }
      }
      className="flex flex-col h-full overflow-y-auto"
    >
      <CatalogShell
        locale={locale}
        loading={loading}
        error={error}
        apps={apps}
        busyIds={busyIds}
        onReload={() => void reload()}
        onOpenApp={(app) => void handleOpen(app)}
        onStartApp={(app) => void handleStart(app)}
        onStopApp={(app) => void handleStop(app)}
        onDeleteApp={setDeleteTarget}
        onRestartApp={(app) => void handleRestart(app)}
        onAppLogs={(app) => logsRef.current?.open(app)}
        onRunSettings={(app) => editRef.current?.open(app)}
        onResolveOrphan={(app, restart) => void handleResolveOrphan(app, restart)}
        onInstallDeps={(app) => void imports.openDepInstall(app, showToast)}
        addMenu={imports.addMenu}
        setAddMenu={imports.setAddMenu}
        onAddImport={handleAddImport}
        onAddLocal={() => setLocalWizardOpen(true)}
        onAddGithub={() => setGithubWizardOpen(true)}
        inbox={
          <ProposalInboxController
            onRegistered={() => {
              void reload();
            }}
            onToast={showToast}
          />
        }
        dock={dock}
      />

      <ModuleImportDialog
        dialog={moduleImport.dialog}
        selected={moduleImport.selected}
        onSelect={moduleImport.onSelect}
        installing={moduleImport.installing}
        onClose={moduleImport.onClose}
        onConfirm={moduleImport.onConfirm}
      />

      <DeleteDialog
        target={deleteTarget}
        onClose={() => setDeleteTarget(null)}
        onDeleted={() => void reload()}
        withBusy={withBusy}
        closeBrowser={windows.closeBrowser}
        onToast={showToast}
      />

      <LogsController ref={logsRef} onToast={showToast} />
      <LocalEditDialog ref={editRef} onToast={showToast} onSaved={() => void reload()} />

      {localWizardOpen && (
        <LocalImportWizard
          open
          onClose={() => setLocalWizardOpen(false)}
          onToast={showToast}
          onSaved={() => {
            setLocalWizardOpen(false);
            void reload();
          }}
        />
      )}

      {githubWizardOpen && (
        <GitHubInstallWizard
          open
          onClose={() => setGithubWizardOpen(false)}
          onToast={showToast}
          onInstalled={() => {
            setGithubWizardOpen(false);
            void reload();
          }}
        />
      )}

      <DepInstallDialog
        depInstallFor={imports.depInstallFor}
        depCommand={imports.depCommand}
        depInstalling={imports.depInstalling}
        depConfirmChecked={imports.depConfirmChecked}
        onSetDepConfirmChecked={imports.setDepConfirmChecked}
        onClose={imports.closeDepInstall}
        onRun={() => {
          void imports.confirmDepInstall(() => {
            void reload();
            showToast(t(locale, 'workshop.installDepsDone'));
          }, showToast);
        }}
      />

      {toast && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 bg-[var(--text)] text-[var(--bg)] px-4 py-2 rounded-lg text-xs font-medium shadow-lg z-50 animate-fade-in">
          {toast}
        </div>
      )}
    </motion.div>
  );
}
