//! Creative import controller (batch 10 CR-1003).
//!
//! Owns the "add" menu (import / local / github entry) and the
//! dependency-install flow for local apps. Extracted from WorkshopPage so the
//! import surface has one real controller instead of inline state + handlers.
//! The module-import wizard itself (permDialog / beginImport) remains in the
//! shell because it spans multiple modals; this hook owns the entry + install.

import { useCallback, useState } from 'react';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export type AddMenu = 'closed' | 'open';

/** Whether a dependency preview is loading / the install is running. */
export interface CreativeDepInstallState {
  depInstallFor: CreativeAppSummary | null;
  depInstalling: boolean;
  depConfirmChecked: boolean;
  depCommand: string;
}

export function useCreativeImport() {
  const [addMenu, setAddMenu] = useState<AddMenu>('closed');
  const [depInstallFor, setDepInstallFor] = useState<CreativeAppSummary | null>(null);
  const [depInstalling, setDepInstalling] = useState(false);
  const [depConfirmChecked, setDepConfirmChecked] = useState(false);
  const [depCommand, setDepCommand] = useState('');

  /** Import menu action: pick a file and hand it to the wizard. */
  const pickAndImport = useCallback(
    async (beginImport: (path: string, name: string) => Promise<void>, onToast: (m: string) => void) => {
      setAddMenu('closed');
      try {
        const files = await window.nativesAPI?.dialog?.pickFiles?.();
        const zip = files?.find((f) => f.toLowerCase().endsWith('.zip')) ?? files?.[0];
        if (!zip) return;
        await beginImport(zip, zip.split('/').pop() || zip);
      } catch (err) {
        onToast(classifyError(err).userMessage);
      }
    },
    [],
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
