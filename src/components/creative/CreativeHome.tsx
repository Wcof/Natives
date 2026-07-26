'use client';

import { useCallback, useMemo, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { useCreativeDrafts } from '@/hooks/useCreativeDrafts';
import type { CreativeAppSummary, CreativeDraft } from '@/lib/tauri-adapter';
import { AssistantStoreProvider } from '@/lib/assistant-workspace';
import { createDefaultGateway } from '@/lib/assistant-gateway';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import Modal from '@/components/ui/Modal';
import { draftActions } from '@/lib/creative-draft';
import CreationComposer from './CreationComposer';
import CreationSession from './CreationSession';
import CreativeCatalog from './CreativeCatalog';
import DraftPreview, { type DraftSoftFailure } from './DraftPreview';

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
  /** 孤儿进程恢复（认领 / 重启）；后端 resolve_orphan 此前无 UI 入口 */
  onResolveOrphan?: (app: CreativeAppSummary, restart: boolean) => void;
  /** 本地项目依赖安装入口 */
  onInstallDeps?: (app: CreativeAppSummary) => void;
}

/** publish 的 moduleId 约束：小写字母数字与连字符（与模块目录名对齐）。 */
function toModuleId(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40) || 'my-creation';
}

/**
 * 个人创意 home: create first, manage second.
 *
 * This component only wires — it holds no business rules of its own. Draft
 * state lives in the host (the database is the single source of truth for which
 * revision is current), catalog state lives in `useCreativeAppCatalog`, and the
 * publish gate lives in Rust.
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
  onResolveOrphan,
  onInstallDeps,
}: CreativeHomeProps) {
  const { drafts, loading: draftsLoading, error: draftsError, reload: reloadDrafts } = useCreativeDrafts();
  const { toast } = useToast();
  // The creation conversation gets its own store: it is a different thread from
  // whatever the assistant workbench has open, and sharing one active-conversation
  // pointer between the two surfaces would make each one steal the other's.
  const gateway = useMemo(() => createDefaultGateway(), []);
  const [activeDraftId, setActiveDraftId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const [previewFeedback, setPreviewFeedback] = useState<string | null>(null);
  const [deleteDraftTarget, setDeleteDraftTarget] = useState<CreativeDraft | null>(null);
  const [publishTarget, setPublishTarget] = useState<CreativeDraft | null>(null);

  const activeDraft: CreativeDraft | null = useMemo(
    () => drafts.find((d) => d.draftId === activeDraftId) ?? null,
    [drafts, activeDraftId],
  );

  const openDrafts = useMemo(
    () => drafts.filter((d) => d.state !== 'archived' && d.state !== 'published'),
    [drafts],
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
        setNotice(classifyError(err).userMessage);
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
      setNotice(classifyError(err).userMessage);
    }
  }, [activeDraftId, reloadDrafts]);

  const deleteDraft = useCallback(async () => {
    if (!deleteDraftTarget) return;
    const target = deleteDraftTarget;
    setDeleteDraftTarget(null);
    try {
      await window.nativesAPI?.creativeDraft?.delete(target.draftId);
      setActiveDraftId((prev) => (prev === target.draftId ? null : prev));
      await reloadDrafts();
      toast(t(locale, 'creative.drafts.deleted'), 'success');
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [deleteDraftTarget, locale, reloadDrafts, toast]);

  const dismissDraft = useCallback(() => setActiveDraftId(null), []);

  const activeActions = activeDraft ? draftActions(activeDraft.state, activeDraft.currentRevision) : null;

  const formatTime = useCallback((iso: string) => {
    try { return new Date(iso).toLocaleString(locale.startsWith('zh') ? 'zh-CN' : 'en-US'); }
    catch { return iso; }
  }, [locale]);

  return (
    <div className="flex flex-col gap-6 p-6">
      <CreationComposer locale={locale} onCreate={createDraft} onImport={onImport} />

      {notice && (
        <p className="text-sm" style={{ color: 'var(--danger)' }} role="alert">
          {notice}
        </p>
      )}

      {/* 草稿列表 — 此前草稿关掉后无任何入口可再打开/删除，只能在 DB 里无限积压 */}
      {draftsError ? (
        <p className="text-sm" style={{ color: 'var(--danger)' }} role="alert">
          {draftsError}
          <button
            type="button"
            className="ml-2 underline"
            onClick={() => void reloadDrafts()}
          >
            {t(locale, 'common.retry')}
          </button>
        </p>
      ) : openDrafts.length > 0 && (
        <section className="rounded-lg border" style={{ borderColor: 'var(--border)' }}>
          <header className="border-b px-4 py-2 text-sm font-medium" style={{ borderColor: 'var(--border)', color: 'var(--text)' }}>
            {t(locale, 'creative.drafts.title')}{draftsLoading ? ' …' : ''}
          </header>
          <ul>
            {openDrafts.map((d) => (
              <li
                key={d.draftId}
                className="flex items-center gap-3 border-b px-4 py-2 text-sm last:border-b-0"
                style={{ borderColor: 'var(--border)' }}
              >
                <button
                  type="button"
                  onClick={() => setActiveDraftId(d.draftId)}
                  className="min-w-0 flex-1 truncate text-left"
                  style={{ color: d.draftId === activeDraftId ? 'var(--accent)' : 'var(--text)' }}
                  title={d.intent || d.name}
                >
                  {d.name || t(locale, 'common.untitled')}
                </button>
                <span className="shrink-0 text-xs" style={{ color: 'var(--text-disabled)' }}>
                  rev {d.currentRevision} · {formatTime(d.updatedAt)}
                </span>
                <button
                  type="button"
                  onClick={() => setActiveDraftId(d.draftId)}
                  className="shrink-0 text-xs"
                  style={{ color: 'var(--accent)' }}
                >
                  {t(locale, 'creative.drafts.open')}
                </button>
                <button
                  type="button"
                  onClick={() => setDeleteDraftTarget(d)}
                  className="shrink-0 text-xs"
                  style={{ color: 'var(--danger)' }}
                >
                  {t(locale, 'common.delete')}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {activeDraft && (
        <section className="rounded-lg border" style={{ borderColor: 'var(--border)' }}>
          <header className="flex items-center justify-between border-b px-4 py-2" style={{ borderColor: 'var(--border)' }}>
            <span className="text-sm font-medium" style={{ color: 'var(--text)' }}>
              {activeDraft.name}
            </span>
            <span className="flex items-center gap-3">
              {/* 发布入口 — 此前 publish 后端完整在线但 UI 零调用方，创作闭环断头 */}
              {activeActions?.canPublish && (
                <button
                  type="button"
                  onClick={() => setPublishTarget(activeDraft)}
                  className="rounded px-3 py-1 text-sm font-medium"
                  style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}
                >
                  {t(locale, 'creative.publish.action')}
                </button>
              )}
              <button
                type="button"
                onClick={dismissDraft}
                className="text-sm"
                style={{ color: 'var(--text-secondary)' }}
              >
                {t(locale, 'creative.closeDraft')}
              </button>
            </span>
          </header>
          <div className="grid h-[460px] grid-cols-1 lg:grid-cols-2">
            <div className="min-h-0 border-b lg:border-b-0 lg:border-r" style={{ borderColor: 'var(--border)' }}>
              <AssistantStoreProvider gateway={gateway}>
                {/* key：草稿切换必须整体重建会话组件，否则旧草稿的 conversationId
                    残留在 state 里，消息会带着旧会话+新 draftId 交叉污染 */}
                <CreationSession
                  key={activeDraft.draftId}
                  draft={activeDraft}
                  locale={locale}
                  onDraftMayHaveChanged={() => void reloadDrafts()}
                  onGeneratingChange={setGenerating}
                  pendingFeedback={previewFeedback}
                  onFeedbackConsumed={() => setPreviewFeedback(null)}
                />
              </AssistantStoreProvider>
            </div>
            <div className="min-h-0">
              <DraftPreview
                draft={activeDraft}
                locale={locale}
                generating={generating}
                onUndo={() => void undoRevision()}
                onSoftFailure={(failure: DraftSoftFailure) => {
                  setPreviewFeedback(failure.detail ? `${failure.reason}: ${failure.detail}` : failure.reason);
                }}
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
        onDelete={onDeleteApp}
        onRestart={onRestartApp}
        onLogs={onAppLogs}
        onRunSettings={onRunSettings}
        onResolveOrphan={onResolveOrphan}
        onInstallDeps={onInstallDeps}
        onCreateNew={() => {
          document.getElementById('creative-intent-input')?.focus();
        }}
        onImport={onImport}
      />

      <ConfirmDialog
        open={deleteDraftTarget !== null}
        title={t(locale, 'creative.drafts.deleteConfirm')}
        message={t(locale, 'creative.drafts.deleteMessage', { name: deleteDraftTarget?.name ?? '' })}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => void deleteDraft()}
        onCancel={() => setDeleteDraftTarget(null)}
      />

      {publishTarget && (
        <PublishDialog
          draft={publishTarget}
          locale={locale}
          onClose={() => setPublishTarget(null)}
          onPublished={() => {
            setPublishTarget(null);
            setActiveDraftId(null);
            void reloadDrafts();
            onReloadApps();
          }}
        />
      )}
    </div>
  );
}

/** 发布对话框：草稿 → 个人创作模块（creativeDraft.publish）。 */
function PublishDialog({ draft, locale, onClose, onPublished }: {
  draft: CreativeDraft;
  locale: Locale;
  onClose: () => void;
  onPublished: () => void;
}) {
  const { toast } = useToast();
  const [name, setName] = useState(draft.name);
  const [moduleId, setModuleId] = useState(draft.originModuleId ?? toModuleId(draft.name));
  const [publishing, setPublishing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const idValid = /^[a-z0-9][a-z0-9-]{1,39}$/.test(moduleId);

  const handlePublish = async () => {
    if (!idValid || !name.trim() || publishing) return;
    const api = window.nativesAPI?.creativeDraft;
    if (!api?.publish) {
      setError(classifyError(new Error('creativeDraft API unavailable')).userMessage);
      return;
    }
    setPublishing(true);
    setError(null);
    try {
      await api.publish({
        draftId: draft.draftId,
        moduleId,
        name: name.trim(),
        permissions: [],
      });
      toast(t(locale, 'creative.publish.success'), 'success');
      onPublished();
    } catch (err) {
      setError(classifyError(err).userMessage);
    } finally {
      setPublishing(false);
    }
  };

  const inputStyle = { borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' } as const;

  return (
    <Modal isOpen onClose={publishing ? () => {} : onClose} title={t(locale, 'creative.publish.title')} width={380}
      closeOnBackdropClick={!publishing} closeOnEscape={!publishing} showCloseButton={!publishing}>
      <div className="space-y-3">
        <div>
          <label className="mb-1 block text-xs" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'creative.publish.name')}
          </label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full rounded border px-3 py-2 text-sm"
            style={inputStyle}
          />
        </div>
        <div>
          <label className="mb-1 block text-xs" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'creative.publish.moduleId')}
          </label>
          <input
            value={moduleId}
            onChange={(e) => setModuleId(e.target.value.toLowerCase())}
            className="w-full rounded border px-3 py-2 font-mono text-sm"
            style={{ ...inputStyle, borderColor: idValid ? 'var(--border)' : 'var(--danger)' }}
            aria-invalid={!idValid}
          />
          {!idValid && (
            <p className="mt-1 text-xs" style={{ color: 'var(--danger)' }}>
              {t(locale, 'creative.publish.moduleIdHint')}
            </p>
          )}
        </div>
        {draft.originModuleId && moduleId === draft.originModuleId && (
          <p className="text-xs" style={{ color: 'var(--warning)' }}>
            {t(locale, 'creative.publish.overwriteHint')}
          </p>
        )}
        {error && <p className="text-sm" style={{ color: 'var(--danger)' }} role="alert">{error}</p>}
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={onClose} disabled={publishing} className="px-4 py-2 text-sm"
          style={{ color: 'var(--text-secondary)' }}>
          {t(locale, 'common.cancel')}
        </button>
        <button type="button" onClick={handlePublish} disabled={!idValid || !name.trim() || publishing}
          className="rounded px-4 py-2 text-sm disabled:opacity-50"
          style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}>
          {publishing ? t(locale, 'creative.publish.publishing') : t(locale, 'creative.publish.action')}
        </button>
      </div>
    </Modal>
  );
}
