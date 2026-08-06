'use client';

import { useCallback, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';

export interface ModuleImportDialogData {
  source: string;
  /** 授权落库用的 module_id（与展示名分离）。 */
  moduleId: string;
  moduleName: string;
  permissions: string[];
}

export interface ModuleImportWizard {
  dialog: ModuleImportDialogData | null;
  selected: ReadonlySet<string>;
  onSelect: (next: Set<string>) => void;
  installing: boolean;
  onClose: () => void;
  onConfirm: () => void;
  /** Read a module zip manifest and open the permission dialog. */
  beginImport: (source: string, fileName: string) => Promise<void>;
}

/**
 * Module-import permission wizard (T10). Reads the manifest's declared
 * permissions, lets the user pick which to grant, then installs + grants.
 */
export function useModuleImportWizard(
  reload: () => Promise<void>,
  showToast: (message: string) => void,
): ModuleImportWizard {
  const locale = useLocale();
  const [dialog, setDialog] = useState<ModuleImportDialogData | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [installing, setInstalling] = useState(false);

  const beginImport = useCallback(
    async (source: string, fileName: string) => {
      try {
        const api = window.nativesAPI;
        const manifest = (await api?.module?.readManifest?.(source)) as
          | { id?: string; name?: string; permissions?: string[] }
          | undefined;
        if (manifest?.id) {
          const perms = manifest.permissions || [];
          setDialog({
            source,
            // 授权按 module_id 落库（module_grant_permission 的键），
            // 传 name 在 id≠name 时会把权限写到不存在的模块上。
            moduleId: manifest.id,
            moduleName: manifest.name || manifest.id,
            permissions: perms,
          });
          setSelected(new Set(perms));
        } else {
          showToast(t(locale, 'workshop.invalidPackage').replace('{name}', fileName));
        }
      } catch (err) {
        showToast(classifyError(err).userMessage);
      }
    },
    [locale, showToast],
  );

  const confirmImport = useCallback(async () => {
    if (!dialog) return;
    setInstalling(true);
    try {
      await window.nativesAPI?.module?.install?.(dialog.source);
      for (const p of selected) {
        await window.nativesAPI?.module?.grantPermission?.(dialog.moduleId, p);
      }
      showToast(t(locale, 'workshop.installSuccess'));
      setDialog(null);
      await reload();
    } catch (err) {
      showToast(classifyError(err).userMessage);
    } finally {
      setInstalling(false);
    }
  }, [dialog, locale, reload, selected, showToast]);

  const onClose = useCallback(() => setDialog(null), []);

  return {
    dialog,
    selected,
    onSelect: setSelected,
    installing,
    onClose,
    onConfirm: () => void confirmImport(),
    beginImport,
  };
}
