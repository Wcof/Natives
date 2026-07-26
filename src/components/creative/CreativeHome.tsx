'use client';

import { useCallback, useMemo, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { useCreativeDrafts } from '@/hooks/useCreativeDrafts';
import type { CreativeAppSummary, CreativeDraft } from '@/lib/tauri-adapter';
import { AssistantStoreProvider } from '@/lib/assistant-workspace';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import CreationComposer from './CreationComposer';
import CreationSession from './CreationSession';
import CreativeCatalog from './CreativeCatalog';
import DraftPreview from './DraftPreview';

interface CreativeHomeProps {
  locale: Locale;
  /**
   * Catalog data is passed in rather than fetched here: the host page already
   * owns `useCreativeAppCatalog` for its lifecycle actions, and a second
   * subscription would mean two lists that can disagree mid-action.
   */
  apps: CreativeAppSummary[];
  busyIds: ReadonlySet<string>;
  onReloadApps: () => void;
  /** Hands off to the existing import flows (GitHub wizard / local project). */
  onImport: () => void;
  /** Opens a published app the way the shell already does. */
  onOpenApp: (app: CreativeAppSummary) => void;
  onStartApp: (app: CreativeAppSummary) => void;
  onStopApp: (app: CreativeAppSummary) => void;
  onDeleteApp: (app: CreativeAppSummary) => void;
  onRestartApp?: (app: CreativeAppSummary) => void;
  onAppLogs?: (app: CreativeAppSummary) => void;
  onRunSettings?: (app: CreativeAppSummary) => void;
}

/**
 * 个人创意 home: create first, manage second.
 *
 * This component only wires — it holds no business rules of its own. Draft
 * state lives in the host (the database is the single source of truth for which
 * revision is current), catalog state lives in `useCreativeAppCatalog`, and the
 * publish gate lives in Rust. Anything that looks like a rule here would be a
 * second copy of one that already exists somewhere authoritative.
 */
export default function CreativeHome({
  locale,
  apps,
  busyIds,
  onReloadApps,
  onImport,
  onOpenApp,
  onStartApp,
  onStopApp,
  onDeleteApp,
  onRestartApp,
  onAppLogs,
  onRunSettings,
}: CreativeHomeProps) {
  const { drafts, reload: reloadDrafts } = useCreativeDrafts();
  // The creation conversation gets its own store: it is a different thread from
  // whatever the assistant workbench has open, and sharing one active-conversation
  // pointer between the two surfaces would make each one steal the other's.
  const gateway = useMemo(() => createDefaultGateway(), []);
  const [activeDraftId, setActiveDraftId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const activeDraft: CreativeDraft | null = useMemo(
    () => drafts.find((d) => d.draftId === activeDraftId) ?? null,
    [drafts, activeDraftId],
  );

  const createDraft = useCallback(
    async (intent: string, name?: string) => {
      const api = window.nativesAPI?.creativeDraft;
      if (!api?.create) throw new Error(t(locale, 'creative.unavailable'));
      const draft = await api.create({
        // Falling back to the intent keeps the card readable when the user
        // skipped the optional name; they can rename it at publish time.
        name: name ?? intent.slice(0, 40),
        intent,
      });
      setActiveDraftId(draft.draftId);
      await reloadDrafts();
    },
    [locale, reloadDrafts],
  );

  /** "Continue creating" seeds a fresh draft from what is currently live. */
  const continueCreating = useCallback(
    async (app: CreativeAppSummary) => {
      const api = window.nativesAPI?.creativeDraft;
      if (!api?.create) {
        setNotice(t(locale, 'creative.unavailable'));
        return;
      }
      try {
        const draft = await api.create({
          name: app.title,
          intent: '',
          originModuleId: app.id,
        });
        setActiveDraftId(draft.draftId);
        await reloadDrafts();
      } catch (err) {
        setNotice(err instanceof Error ? err.message : String(err));
      }
    },
    [locale, reloadDrafts],
  );

  const undoRevision = useCallback(async () => {
    if (!activeDraftId) return;
    const api = window.nativesAPI?.creativeDraft;
    if (!api?.rollback) return;
    try {
      await api.rollback(activeDraftId);
      await reloadDrafts();
    } catch (err) {
      setNotice(err instanceof Error ? err.message : String(err));
    }
  }, [activeDraftId, reloadDrafts]);

  const dismissDraft = useCallback(() => setActiveDraftId(null), []);

  return (
    <div className="flex flex-col gap-6 p-6">
      <CreationComposer locale={locale} onCreate={createDraft} onImport={onImport} />

      {notice && (
        <p className="text-sm text-red-600 dark:text-red-400" role="alert">
          {notice}
        </p>
      )}

      {activeDraft && (
        <section className="rounded-lg border border-neutral-200 dark:border-neutral-800">
          <header className="flex items-center justify-between border-b border-neutral-200 px-4 py-2 dark:border-neutral-800">
            <span className="text-sm font-medium text-neutral-900 dark:text-neutral-100">
              {activeDraft.name}
            </span>
            <button
              type="button"
              onClick={dismissDraft}
              className="text-sm text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100"
            >
              {t(locale, 'creative.closeDraft')}
            </button>
          </header>
          <div className="grid h-[460px] grid-cols-1 lg:grid-cols-2">
            <div className="min-h-0 border-b border-neutral-200 lg:border-b-0 lg:border-r dark:border-neutral-800">
              <AssistantStoreProvider gateway={gateway}>
                <CreationSession
                  draft={activeDraft}
                  locale={locale}
                  onDraftMayHaveChanged={() => void reloadDrafts()}
                />
              </AssistantStoreProvider>
            </div>
            <div className="min-h-0">
              <DraftPreview
                draft={activeDraft}
                locale={locale}
                onUndo={() => void undoRevision()}
              />
            </div>
          </div>
        </section>
      )}

      <CreativeCatalog
        apps={apps}
        locale={locale}
        busyIds={busyIds}
        onOpen={onOpenApp}
        onContinueCreating={(app) => void continueCreating(app)}
        onStart={onStartApp}
        onStop={onStopApp}
        onDelete={(app) => {
          onDeleteApp(app);
          onReloadApps();
        }}
        onRestart={onRestartApp}
        onLogs={onAppLogs}
        onRunSettings={onRunSettings}
        onCreateNew={() => {
          document.querySelector('textarea')?.focus();
        }}
        onImport={onImport}
      />
    </div>
  );
}
