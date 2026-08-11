//! Creative import controller (batch 10 CR-1003).
//!
//! Owns the "add" menu (import / local / github entry) and the
//! dependency-install flow for local apps. Extracted from WorkshopPage so the
//! import surface has one real controller instead of inline state + handlers.
//! The module-import wizard itself (permDialog / beginImport) remains in the
//! shell because it spans multiple modals; this hook owns the entry + install.

import { useCallback, useState } from 'react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { basename, dirname } from '@/lib/path-utils';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export type AddMenu = 'closed' | 'open';

/** Whether a dependency preview is loading / the install is running. */
export interface CreativeDepInstallState {
  depInstallFor: CreativeAppSummary | null;
  depInstalling: boolean;
  depConfirmChecked: boolean;
  depCommand: string;
}

/** Pre-filled local-project wizard state for an HTML import (问题13). */
export interface HtmlImportInitial {
  /** Parent directory of the selected HTML file. */
  root: string;
  /** File name relative to root (validated under root by the daemon). */
  entryFile: string;
  title: string;
}

/**
 * Route one picked file by extension (问题13): `.zip` → Workshop module
 * install; `.html/.htm` → local project static run; anything else is not
 * importable. Windows separators are normalized before matching.
 */
export function routeImportFile(filePath: string): 'zip' | 'html' | 'other' {
  const posix = filePath.replace(/\\/g, '/').toLowerCase();
  if (posix.endsWith('.zip')) return 'zip';
  if (posix.endsWith('.html') || posix.endsWith('.htm')) return 'html';
  return 'other';
}

/** Derive the local-wizard prefill for a picked HTML file. */
export function htmlImportInitial(filePath: string): HtmlImportInitial {
  const posix = filePath.replace(/\\/g, '/');
  const entryFile = basename(posix);
  const root = dirname(posix);
  return { root, entryFile, title: entryFile };
}

export function useCreativeImport() {
  const locale = useLocale();
  const [addMenu, setAddMenu] = useState<AddMenu>('closed');
  const [depInstallFor, setDepInstallFor] = useState<CreativeAppSummary | null>(null);
  const [depInstalling, setDepInstalling] = useState(false);
  const [depConfirmChecked, setDepConfirmChecked] = useState(false);
  const [depCommand, setDepCommand] = useState('');

  /**
   * Import menu action: pick ONE file and route it by type (问题13):
   * `.zip` → Workshop module manifest/install; `.html/.htm` → local project
   * wizard prefilled with parent dir + relative entry file; anything else
   * shows the supported-format hint. Dialog cancel returns without error.
   */
  const pickAndImport = useCallback(
    async (
      beginImport: (path: string, name: string) => Promise<void>,
      onToast: (m: string) => void,
      onOpenLocalHtml?: (initial: HtmlImportInitial) => void,
    ) => {
      setAddMenu('closed');
      try {
        const files = await window.nativesAPI?.dialog?.pickFiles?.();
        const selected = files?.[0];
        if (!selected) return; // user cancel — silent
        const kind = routeImportFile(selected);
        if (kind === 'zip') {
          await beginImport(selected, basename(selected.replace(/\\/g, '/')) || selected);
          return;
        }
        if (kind === 'html') {
          if (onOpenLocalHtml) onOpenLocalHtml(htmlImportInitial(selected));
          else onToast(t(locale, 'workshop.importUnsupported'));
          return;
        }
        onToast(t(locale, 'workshop.importUnsupported'));
      } catch (err) {
        // Dialog adapter no longer swallows real plugin/permission errors;
        // surface them through the unified toast (问题13).
        onToast(classifyError(err).userMessage);
      }
    },
    [locale],
  );

  /** Open the dependency-install dialog and fetch the preview command. */
  const openDepInstall = useCallback(
    async (app: CreativeAppSummary, onToast: (m: string) => void) => {
      setDepConfirmChecked(false);
      setDepCommand('');
      setDepInstallFor(app);
      try {
        const preview =
          await window.nativesAPI?.creativeApp?.previewLocalDependencyInstall?.(app.id);
        if (preview) setDepCommand([preview.program, ...preview.args].join(' '));
      } catch (err) {
        onToast(classifyError(err).userMessage);
      }
    },
    [],
  );

  /** Confirm-and-run the dependency install. On success clears the dialog. */
  const confirmDepInstall = useCallback(
    async (onDone: () => void, onToast: (m: string) => void) => {
      if (!depInstallFor) return;
      setDepInstalling(true);
      try {
        await window.nativesAPI?.creativeApp?.installLocalDependencies?.(depInstallFor.id);
        setDepInstallFor(null);
        onDone();
      } catch (err) {
        onToast(classifyError(err).userMessage);
      } finally {
        setDepInstalling(false);
      }
    },
    [depInstallFor],
  );

  /** Dismiss the dependency-install dialog (cancel / close). */
  const closeDepInstall = useCallback(() => {
    setDepInstallFor(null);
    setDepInstalling(false);
  }, []);

  return {
    addMenu,
    setAddMenu,
    pickAndImport,
    openDepInstall,
    confirmDepInstall,
    closeDepInstall,
    depInstallFor,
    depInstalling,
    depConfirmChecked,
    setDepConfirmChecked,
    depCommand,
  };
}
