'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import {
  ChevronLeft,
  ChevronRight,
  Github,
  Layers,
  Package,
  Pause,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  ScrollText,
  Trash2,
  X,
} from 'lucide-react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import { useCreativeAppCatalog } from '@/hooks/useCreativeAppCatalog';
import {
  defaultDeleteOptions,
  isActionBusy,
  mergeActionsWithBusy,
  sourceBadge,
} from '@/lib/creative-app';
import type {
  CreativeAppBrowserBounds,
  CreativeAppInspectResult,
  CreativeAppInstallCandidate,
  CreativeAppProgressEvent,
  CreativeAppSummary,
} from '@/lib/tauri-adapter';

interface WorkshopPageProps {
  onInstall: (source: string) => void;
}

type AddMenu = 'closed' | 'open';
type WizardStep = 'url' | 'manual' | 'installing';

function stateLabel(locale: Locale, state: CreativeAppSummary['state']): string {
  const key = {
    available: 'workshop.stateAvailable',
    disabled: 'workshop.stateDisabled',
    installing: 'workshop.stateInstalling',
    installed_stopped: 'workshop.stateInstalledStopped',
    starting: 'workshop.stateStarting',
    running: 'workshop.stateRunning',
    stopping: 'workshop.stateStopping',
    runtime_unavailable: 'workshop.stateRuntimeUnavailable',
    install_failed: 'workshop.stateInstallFailed',
    start_failed: 'workshop.stateStartFailed',
    deleting: 'workshop.stateDeleting',
    delete_failed: 'workshop.stateDeleteFailed',
  }[state];
  return t(locale, key);
}

function runtimeLabel(locale: Locale, runtime: CreativeAppSummary['runtime']): string {
  if (runtime === 'docker_compose') return t(locale, 'workshop.runtimeCompose');
  if (runtime === 'docker_run') return t(locale, 'workshop.runtimeRun');
  return t(locale, 'workshop.runtimeWorkshop');
}

export default function WorkshopPage({ onInstall }: WorkshopPageProps) {
  void onInstall;
  const prefersReducedMotion = useReducedMotion();
  const { apps, loading, error, reload, busyIds, withBusy } = useCreativeAppCatalog();
  const [locale, setLocale] = useState<Locale>('zh');
  const [toast, setToast] = useState<string | null>(null);
  const [addMenu, setAddMenu] = useState<AddMenu>('closed');

  // Internal create / import (existing flows)
  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [templateName, setTemplateName] = useState('');
  const [templateId, setTemplateId] = useState('');
  const [creating, setCreating] = useState(false);
  const [permDialog, setPermDialog] = useState<{
    source: string;
    moduleName: string;
    permissions: string[];
  } | null>(null);
  const [selectedPerms, setSelectedPerms] = useState<Set<string>>(new Set());
  const [installing, setInstalling] = useState(false);
  const [dragOver, setDragOver] = useState(false);

  // Delete
  const [deleteTarget, setDeleteTarget] = useState<CreativeAppSummary | null>(null);
  const [deleteVolumes, setDeleteVolumes] = useState(false);
  const [deleteImages, setDeleteImages] = useState(false);

  // GitHub wizard
  const [wizardOpen, setWizardOpen] = useState(false);
  const [wizardStep, setWizardStep] = useState<WizardStep>('url');
  const [repoUrl, setRepoUrl] = useState('');
  const [tokenMode, setTokenMode] = useState<'saved' | 'once' | 'public'>('public');
  const [tokenInput, setTokenInput] = useState('');
  const [saveToken, setSaveToken] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [inspect, setInspect] = useState<CreativeAppInspectResult | null>(null);
  const [selectedTag, setSelectedTag] = useState('');
  const [selectedCandidate, setSelectedCandidate] = useState<CreativeAppInstallCandidate | null>(null);
  const [hostPort, setHostPort] = useState('');
  const [openPath, setOpenPath] = useState('/');
  const [healthPath, setHealthPath] = useState('');
  const [service, setService] = useState('');
  const [envValues, setEnvValues] = useState<Record<string, string>>({});
  const [confirmBinds, setConfirmBinds] = useState(false);
  const [progress, setProgress] = useState<CreativeAppProgressEvent | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  // External browser surface
  const [browserApp, setBrowserApp] = useState<CreativeAppSummary | null>(null);
  const [browserUrl, setBrowserUrl] = useState('');
  const browserHostRef = useRef<HTMLDivElement | null>(null);

  // Logs
  const [logsFor, setLogsFor] = useState<CreativeAppSummary | null>(null);
  const [logsText, setLogsText] = useState('');

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2400);
  }, []);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch {
        /* browser dev */
      }
    }
    loadLocale();
  }, []);

  useEffect(() => {
    const api = window.nativesAPI?.creativeApp;
    if (!api?.onProgress) return;
    return api.onProgress((ev) => setProgress(ev));
  }, []);

  // Report browser bounds
  useEffect(() => {
    if (!browserApp) return;
    const el = browserHostRef.current;
    if (!el) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      const bounds: CreativeAppBrowserBounds = {
        x: r.left,
        y: r.top,
        width: r.width,
        height: r.height,
      };
      void window.nativesAPI?.creativeApp?.browserSetBounds?.(bounds);
    };
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    window.addEventListener('resize', report);
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', report);
    };
  }, [browserApp]);

  useEffect(() => {
    return () => {
      void window.nativesAPI?.creativeApp?.browserClose?.();
    };
  }, []);

  const openInternalModule = async (id: string) => {
    window.dispatchEvent(new CustomEvent('navigate', { detail: `module:${id}` }));
  };

  const openExternal = async (app: CreativeAppSummary) => {
    try {
      const target = await window.nativesAPI?.creativeApp?.getOpenTarget?.(app.id);
      if (!target || target.kind !== 'local_url') {
        showToast(t(locale, 'workshop.stateStartFailed'));
        return;
      }
      const el = browserHostRef.current;
      // ensure state first so ref mounts
      setBrowserApp(app);
      setBrowserUrl(target.url);
      // next frame for layout
      requestAnimationFrame(() => {
        const host = browserHostRef.current ?? el;
        const r = host?.getBoundingClientRect();
        const bounds: CreativeAppBrowserBounds = r
          ? { x: r.left, y: r.top, width: r.width, height: r.height }
          : { x: 280, y: 80, width: 900, height: 640 };
        void window.nativesAPI?.creativeApp?.browserShow?.(app.id, target.url, bounds);
      });
    } catch (err) {
      showToast(classifyError(err).userMessage);
    }
  };

  const closeBrowser = async () => {
    await window.nativesAPI?.creativeApp?.browserClose?.();
    setBrowserApp(null);
    setBrowserUrl('');
  };

  const handleOpen = async (app: CreativeAppSummary) => {
    if (app.source === 'internal') {
      await openInternalModule(app.id);
    } else {
      await openExternal(app);
    }
  };

  const handleStart = async (app: CreativeAppSummary) => {
    if (busyIds.has(app.id)) return;
    await withBusy(app.id, async () => {
      try {
        if (app.source === 'internal') {
          await window.nativesAPI?.module?.enable?.(app.id);
        } else {
          await window.nativesAPI?.creativeApp?.start?.(app.id);
        }
      } catch (err) {
        showToast(classifyError(err).userMessage);
      }
    });
  };

  const handleStop = async (app: CreativeAppSummary) => {
    if (busyIds.has(app.id)) return;
    if (browserApp?.id === app.id) await closeBrowser();
    await withBusy(app.id, async () => {
      try {
        if (app.source === 'internal') {
          await window.nativesAPI?.module?.disable?.(app.id);
        } else {
          await window.nativesAPI?.creativeApp?.stop?.(app.id);
        }
      } catch (err) {
        showToast(classifyError(err).userMessage);
      }
    });
  };

  const doDelete = async () => {
    if (!deleteTarget) return;
    const id = deleteTarget.id;
    if (browserApp?.id === id) await closeBrowser();
    await withBusy(id, async () => {
      try {
        if (deleteTarget.source === 'internal') {
          await window.nativesAPI?.module?.uninstall?.(id);
        } else {
          await window.nativesAPI?.creativeApp?.delete?.(id, {
            removeVolumes: deleteVolumes,
            removeImages: deleteImages,
          });
        }
      } catch (err) {
        showToast(classifyError(err).userMessage);
      } finally {
        setDeleteTarget(null);
        setDeleteVolumes(false);
        setDeleteImages(false);
      }
    });
  };

  const openLogs = async (app: CreativeAppSummary) => {
    setLogsFor(app);
    setLogsText('…');
    try {
      const text = await window.nativesAPI?.creativeApp?.logs?.(app.id, 200);
      setLogsText(text || '');
    } catch (err) {
      setLogsText(classifyError(err).userMessage);
    }
  };

  // ── Import local package ──
  const beginImport = async (source: string, fileName: string) => {
    try {
      const api = window.nativesAPI;
      const result = (await api?.module?.readManifest?.(source)) as
        | { manifest?: { name: string; permissions: string[] }; error?: string }
        | undefined;
      if (result?.manifest) {
        const perms = result.manifest.permissions || [];
        setPermDialog({
          source,
          moduleName: result.manifest.name,
          permissions: perms,
        });
        setSelectedPerms(new Set(perms));
      } else {
        showToast(
          result?.error
            ? t(locale, 'errors.installFailed').replace('{reason}', result.error)
            : t(locale, 'workshop.invalidPackage').replace('{name}', fileName),
        );
      }
    } catch (err) {
      showToast(
        t(locale, 'errors.installFailed').replace(
          '{reason}',
          classifyError(err).userMessage,
        ),
      );
    }
  };

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    const files = Array.from(e.dataTransfer.files);
    for (const file of files) {
      if (file.name.endsWith('.zip') || file.type === '') {
        const source = (file as { path?: string }).path || file.name;
        await beginImport(source, file.name);
      }
    }
  };

  const confirmImport = async () => {
    if (!permDialog) return;
    setInstalling(true);
    try {
      await window.nativesAPI?.module?.install?.(permDialog.source);
      for (const p of selectedPerms) {
        await window.nativesAPI?.module?.grantPermission?.(
          // module id unknown until install — approve path still works after list refresh
          permDialog.moduleName,
          p,
        );
      }
      showToast(t(locale, 'workshop.installSuccess'));
      setPermDialog(null);
      await reload();
    } catch (err) {
      showToast(
        t(locale, 'workshop.installFailed') + ': ' + classifyError(err).userMessage,
      );
    } finally {
      setInstalling(false);
    }
  };

  const createTemplate = async () => {
    if (!templateName.trim() || !templateId.trim()) return;
    setCreating(true);
    try {
      const html = `<!DOCTYPE html><html><head><meta charset="utf-8"/><title>${templateName}</title></head><body><h1>${templateName}</h1><p>Generated by Natives Personal Creations.</p></body></html>`;
      await window.nativesAPI?.module?.writeGenerated?.(
        templateId.trim(),
        templateName.trim(),
        html,
        [],
      );
      showToast(t(locale, 'workshop.templateCreated'));
      setShowCreateDialog(false);
      setTemplateName('');
      setTemplateId('');
      await reload();
    } catch (err) {
      showToast(
        t(locale, 'workshop.templateFailed') + ': ' + classifyError(err).userMessage,
      );
    } finally {
      setCreating(false);
    }
  };

  // ── GitHub wizard ──
  const resetWizard = () => {
    setWizardStep('url');
    setRepoUrl('');
    setTokenInput('');
    setSaveToken(false);
    setInspect(null);
    setSelectedCandidate(null);
    setSelectedTag('');
    setHostPort('');
    setOpenPath('/');
    setHealthPath('');
    setService('');
    setEnvValues({});
    setConfirmBinds(false);
    setProgress(null);
    setInstallError(null);
  };

  const tokenForRequest = (): string | undefined => {
    if (tokenMode === 'once' && tokenInput.trim()) return tokenInput.trim();
    return undefined;
  };

  const runInspect = async (oneClick: boolean) => {
    setInspecting(true);
    setInstallError(null);
    try {
      const result = await window.nativesAPI?.creativeApp?.inspectGithub?.({
        repositoryUrl: repoUrl.trim(),
        token: tokenForRequest(),
        saveToken: saveToken && tokenMode === 'once',
        oneClick,
        releaseTag: oneClick ? null : selectedTag || null,
      });
      if (!result) throw new Error('inspect failed');
      setInspect(result);
      setSelectedTag(result.releaseTag);
      const top = result.candidates[0] ?? null;
      setSelectedCandidate(top);
      if (top) {
        setHostPort(top.suggestedHostPort ? String(top.suggestedHostPort) : '');
        setOpenPath(top.openPath || '/');
        setHealthPath(top.healthPath || '');
        setService(top.service || '');
        const env: Record<string, string> = {};
        for (const e of top.envRequirements) env[e.key] = '';
        setEnvValues(env);
      }
      if (oneClick && result.oneClickEligible && result.oneClickCandidateId) {
        const cand =
          result.candidates.find((c) => c.id === result.oneClickCandidateId) || top;
        if (cand) {
          await runInstall(result, cand, true);
          return;
        }
      }
      setWizardStep('manual');
    } catch (err) {
      setInstallError(classifyError(err).userMessage);
      setWizardStep('manual');
    } finally {
      setInspecting(false);
      // clear one-shot token from UI state after use
      if (tokenMode === 'once') setTokenInput('');
    }
  };

  const runInstall = async (
    ins: CreativeAppInspectResult,
    cand: CreativeAppInstallCandidate,
    fromOneClick: boolean,
  ) => {
    setWizardStep('installing');
    setInstallError(null);
    try {
      const summary = await window.nativesAPI?.creativeApp?.installGithub?.({
        repositoryUrl: ins.repositoryUrl,
        releaseTag: selectedTag || ins.releaseTag,
        releaseId: ins.releaseId,
        candidateId: cand.id,
        token: tokenForRequest(),
        hostPort: hostPort ? Number(hostPort) : cand.suggestedHostPort,
        openPath: openPath || cand.openPath,
        healthPath: healthPath || cand.healthPath,
        service: service || cand.service,
        env: Object.entries(envValues).map(([key, value]) => ({ key, value })),
        confirmBindMounts: confirmBinds || fromOneClick,
      });
      setTokenInput('');
      setWizardOpen(false);
      resetWizard();
      await reload();
      if (summary) {
        showToast(t(locale, 'workshop.installSuccess'));
        if (summary.state === 'running') {
          await openExternal(summary);
        }
      }
    } catch (err) {
      setInstallError(classifyError(err).userMessage);
      setWizardStep('manual');
    }
  };

  const stageLabel = (stage: string) => {
    const map: Record<string, string> = {
      inspecting_release: 'workshop.githubStageInspect',
      downloading_assets: 'workshop.githubStageDownload',
      pulling_image: 'workshop.githubStagePull',
      creating: 'workshop.githubStageCreate',
      starting: 'workshop.githubStageStart',
      health_check: 'workshop.githubStageHealth',
      ready: 'workshop.githubStageReady',
      failed: 'workshop.githubStageFailed',
    };
    return t(locale, map[stage] || 'workshop.githubStageInstall');
  };

  // If browser open, render browser chrome over list
  if (browserApp) {
    return (
      <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '8px 12px',
            borderBottom: '1px solid var(--border)',
            background: 'var(--surface)',
          }}
        >
          <button type="button" className="btn btn-ghost" onClick={() => void window.nativesAPI?.creativeApp?.browserBack?.()} title={t(locale, 'workshop.browserBack')}>
            <ChevronLeft size={14} />
          </button>
          <button type="button" className="btn btn-ghost" onClick={() => void window.nativesAPI?.creativeApp?.browserForward?.()} title={t(locale, 'workshop.browserForward')}>
            <ChevronRight size={14} />
          </button>
          <button type="button" className="btn btn-ghost" onClick={() => void window.nativesAPI?.creativeApp?.browserReload?.()} title={t(locale, 'workshop.browserReload')}>
            <RefreshCw size={14} />
          </button>
          <div
            style={{
              flex: 1,
              fontSize: FONT_SIZE.xs,
              fontFamily: 'var(--font-mono)',
              color: 'var(--text-secondary)',
              padding: '4px 8px',
              border: '1px solid var(--border)',
              borderRadius: BORDER_RADIUS.md,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={browserUrl}
          >
            {browserUrl || t(locale, 'workshop.browserAddress')}
          </div>
          <button type="button" className="btn btn-secondary" onClick={() => void closeBrowser()}>
            {t(locale, 'workshop.browserBackToList')}
          </button>
        </div>
        <div ref={browserHostRef} style={{ flex: 1, minHeight: 0, background: 'var(--background)' }} />
      </div>
    );
  }

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 60, damping: 16, mass: 1 }}
      style={{ height: '100%', overflow: 'auto' }}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleDrop}
    >
      <div
        style={{
          padding: `${SPACING.md}px ${SPACING.xl}px`,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid var(--border)',
        }}
      >
        <div>
          <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)' }}>
            {t(locale, 'workshop.title')}
          </div>
          <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', marginTop: 2 }}>
            {t(locale, 'workshop.subtitle')}
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8, position: 'relative' }}>
          <button type="button" className="btn btn-ghost" onClick={() => void reload()} title="Refresh">
            <RefreshCw size={14} />
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setAddMenu((m) => (m === 'open' ? 'closed' : 'open'))}
          >
            <Plus size={14} /> {t(locale, 'workshop.add')}
          </button>
          {addMenu === 'open' && (
            <div
              style={{
                position: 'absolute',
                right: 0,
                top: '110%',
                minWidth: 220,
                background: 'var(--surface)',
                border: '1px solid var(--border)',
                borderRadius: BORDER_RADIUS.md,
                boxShadow: '0 8px 24px rgba(0,0,0,.12)',
                zIndex: 20,
                padding: 6,
              }}
            >
              <button
                type="button"
                className="btn btn-ghost"
                style={{ width: '100%', justifyContent: 'flex-start' }}
                onClick={() => {
                  setAddMenu('closed');
                  setShowCreateDialog(true);
                }}
              >
                <Layers size={14} /> {t(locale, 'workshop.addMenuCreate')}
              </button>
              <label
                className="btn btn-ghost"
                style={{ width: '100%', justifyContent: 'flex-start', cursor: 'pointer' }}
              >
                <Package size={14} /> {t(locale, 'workshop.addMenuImport')}
                <input
                  type="file"
                  accept=".zip"
                  style={{ display: 'none' }}
                  onChange={async (e) => {
                    setAddMenu('closed');
                    const f = e.target.files?.[0];
                    if (!f) return;
                    const source = (f as { path?: string }).path || f.name;
                    await beginImport(source, f.name);
                  }}
                />
              </label>
              <button
                type="button"
                className="btn btn-ghost"
                style={{ width: '100%', justifyContent: 'flex-start' }}
                onClick={() => {
                  setAddMenu('closed');
                  resetWizard();
                  setWizardOpen(true);
                }}
              >
                <Github size={14} /> {t(locale, 'workshop.addMenuGithub')}
              </button>
            </div>
          )}
        </div>
      </div>

      {dragOver && (
        <div
          style={{
            margin: SPACING.xl,
            padding: 32,
            border: '2px dashed var(--primary)',
            borderRadius: BORDER_RADIUS.lg,
            textAlign: 'center',
            color: 'var(--primary)',
          }}
        >
          {t(locale, 'workshop.releaseToInstall')}
        </div>
      )}

      <div style={{ padding: SPACING.xl }}>
        {loading && <LoadingState />}
        {error && (
          <EmptyState
            title={t(locale, 'common.error')}
            description={typeof error === 'string' ? error : classifyError(error).userMessage}
            action={{ label: t(locale, 'common.retry'), onClick: () => { void reload(); } }}
          />
        )}
        {!loading && !error && apps.length === 0 && (
          <EmptyState
            title={t(locale, 'workshop.emptyState')}
            description={t(locale, 'workshop.emptyUnified')}
          />
        )}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))',
            gap: 12,
          }}
        >
          {apps.map((app) => {
            const busy = busyIds.has(app.id) || isActionBusy(app.state);
            const actions = mergeActionsWithBusy(app.actions, busy);
            const badge = sourceBadge(app.source);
            return (
              <div
                key={app.id}
                style={{
                  border: '1px solid var(--border)',
                  borderRadius: BORDER_RADIUS.lg,
                  padding: 14,
                  background: 'var(--surface)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
                  <div style={{ fontWeight: 600, color: 'var(--text)' }}>{app.title}</div>
                  <span
                    style={{
                      fontSize: 10,
                      padding: '2px 8px',
                      borderRadius: 999,
                      border: '1px solid var(--border)',
                      color: 'var(--text-secondary)',
                    }}
                  >
                    {badge === 'github'
                      ? t(locale, 'workshop.sourceGithub')
                      : t(locale, 'workshop.sourceInternal')}
                  </span>
                </div>
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>
                  {runtimeLabel(locale, app.runtime)} · {app.version} · {stateLabel(locale, app.state)}
                </div>
                {app.lastError && (
                  <div style={{ fontSize: 11, color: 'var(--danger)' }}>{app.lastError}</div>
                )}
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 'auto' }}>
                  {actions.canOpen && (
                    <button type="button" className="btn btn-primary" onClick={() => void handleOpen(app)}>
                      {t(locale, 'workshop.actionOpen')}
                    </button>
                  )}
                  {actions.canStart && (
                    <button type="button" className="btn btn-secondary" onClick={() => void handleStart(app)}>
                      <Play size={12} /> {t(locale, 'workshop.actionStart')}
                    </button>
                  )}
                  {actions.canStop && (
                    <button type="button" className="btn btn-secondary" onClick={() => void handleStop(app)}>
                      <Pause size={12} /> {t(locale, 'workshop.actionStop')}
                    </button>
                  )}
                  {actions.canRetry && (
                    <button type="button" className="btn btn-secondary" onClick={() => void handleStart(app)}>
                      <RotateCcw size={12} /> {t(locale, 'workshop.actionRetry')}
                    </button>
                  )}
                  {app.source === 'external_github' && (
                    <button type="button" className="btn btn-ghost" onClick={() => void openLogs(app)}>
                      <ScrollText size={12} /> {t(locale, 'workshop.actionLogs')}
                    </button>
                  )}
                  {actions.canDelete && (
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => {
                        setDeleteTarget(app);
                        const d = defaultDeleteOptions();
                        setDeleteVolumes(d.removeVolumes);
                        setDeleteImages(d.removeImages);
                      }}
                    >
                      <Trash2 size={12} /> {t(locale, 'workshop.actionDelete')}
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {toast && (
        <div
          style={{
            position: 'fixed',
            bottom: 24,
            left: '50%',
            transform: 'translateX(-50%)',
            background: 'var(--text)',
            color: 'var(--bg)',
            padding: '8px 14px',
            borderRadius: 8,
            fontSize: 12,
            zIndex: 50,
          }}
        >
          {toast}
        </div>
      )}

      {/* Create internal */}
      {showCreateDialog && (
        <Modal
          isOpen
          onClose={() => setShowCreateDialog(false)}
          title={t(locale, 'workshop.createModule')}
          width={420}
        >
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <label style={{ fontSize: 12 }}>
              {t(locale, 'workshop.templateName')}
              <input
                value={templateName}
                onChange={(e) => setTemplateName(e.target.value)}
                placeholder={t(locale, 'workshop.templateNamePlaceholder')}
                style={inputStyle}
              />
            </label>
            <label style={{ fontSize: 12 }}>
              {t(locale, 'workshop.templateId')}
              <input
                value={templateId}
                onChange={(e) => setTemplateId(e.target.value)}
                placeholder={t(locale, 'workshop.templateIdPlaceholder')}
                style={inputStyle}
              />
            </label>
            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <button type="button" className="btn btn-secondary" onClick={() => setShowCreateDialog(false)}>
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="btn btn-primary"
                disabled={creating}
                onClick={() => void createTemplate()}
              >
                {creating ? t(locale, 'workshop.creating') : t(locale, 'workshop.createTemplate')}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* Permission dialog */}
      {permDialog && (
        <Modal
          isOpen
          onClose={() => setPermDialog(null)}
          title={t(locale, 'workshop.permissionTitle')}
          width={440}
        >
          <p style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
            {t(locale, 'workshop.permissionDesc').replace('{name}', permDialog.moduleName)}
          </p>
          <ul style={{ fontSize: 12, margin: '12px 0' }}>
            {permDialog.permissions.map((p) => (
              <li key={p}>
                <label>
                  <input
                    type="checkbox"
                    checked={selectedPerms.has(p)}
                    onChange={(e) => {
                      setSelectedPerms((prev) => {
                        const n = new Set(prev);
                        if (e.target.checked) n.add(p);
                        else n.delete(p);
                        return n;
                      });
                    }}
                  />{' '}
                  {p}
                </label>
              </li>
            ))}
          </ul>
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <button type="button" className="btn btn-secondary" onClick={() => setPermDialog(null)}>
              {t(locale, 'common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={installing}
              onClick={() => void confirmImport()}
            >
              {t(locale, 'workshop.permissionAllowAll')}
            </button>
          </div>
        </Modal>
      )}

      {/* Delete dialog */}
      {deleteTarget && (
        <Modal
          isOpen
          onClose={() => setDeleteTarget(null)}
          title={t(locale, 'workshop.deleteTitle')}
          width={420}
        >
          <p style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
            {t(locale, 'workshop.deleteDesc')}
          </p>
          <p style={{ fontSize: 13, fontWeight: 600 }}>{deleteTarget.title}</p>
          {deleteTarget.source === 'external_github' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginTop: 10 }}>
              <label style={{ fontSize: 12 }}>
                <input
                  type="checkbox"
                  checked={deleteVolumes}
                  onChange={(e) => setDeleteVolumes(e.target.checked)}
                />{' '}
                {t(locale, 'workshop.deleteRemoveVolumes')}
              </label>
              <label style={{ fontSize: 12 }}>
                <input
                  type="checkbox"
                  checked={deleteImages}
                  onChange={(e) => setDeleteImages(e.target.checked)}
                />{' '}
                {t(locale, 'workshop.deleteRemoveImages')}
              </label>
            </div>
          )}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 16 }}>
            <button type="button" className="btn btn-secondary" onClick={() => setDeleteTarget(null)}>
              {t(locale, 'common.cancel')}
            </button>
            <button type="button" className="btn btn-primary" onClick={() => void doDelete()}>
              {t(locale, 'workshop.deleteConfirm')}
            </button>
          </div>
        </Modal>
      )}

      {/* Logs */}
      {logsFor && (
        <Modal isOpen onClose={() => setLogsFor(null)} title={t(locale, 'workshop.logsTitle')} width={640}>
          <pre
            style={{
              maxHeight: 360,
              overflow: 'auto',
              fontSize: 11,
              fontFamily: 'var(--font-mono)',
              background: 'var(--bg-2)',
              padding: 12,
              borderRadius: 8,
            }}
          >
            {logsText}
          </pre>
        </Modal>
      )}

      {/* GitHub wizard */}
      {wizardOpen && (
        <Modal
          isOpen
          onClose={() => {
            setWizardOpen(false);
            resetWizard();
          }}
          title={t(locale, 'workshop.githubWizardTitle')}
          width={520}
        >
          {wizardStep === 'url' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              <label style={{ fontSize: 12 }}>
                {t(locale, 'workshop.githubRepoUrl')}
                <input
                  value={repoUrl}
                  onChange={(e) => setRepoUrl(e.target.value)}
                  placeholder={t(locale, 'workshop.githubRepoPlaceholder')}
                  style={inputStyle}
                />
              </label>
              <div style={{ display: 'flex', gap: 8, fontSize: 12 }}>
                {(['public', 'saved', 'once'] as const).map((m) => (
                  <label key={m}>
                    <input
                      type="radio"
                      checked={tokenMode === m}
                      onChange={() => setTokenMode(m)}
                    />{' '}
                    {m === 'public'
                      ? t(locale, 'workshop.githubPublicAccess')
                      : m === 'saved'
                        ? t(locale, 'workshop.githubUseSavedToken')
                        : t(locale, 'workshop.githubOneShotToken')}
                  </label>
                ))}
              </div>
              {tokenMode === 'once' && (
                <>
                  <input
                    type="password"
                    value={tokenInput}
                    onChange={(e) => setTokenInput(e.target.value)}
                    placeholder={t(locale, 'workshop.githubTokenPlaceholder')}
                    style={inputStyle}
                  />
                  <label style={{ fontSize: 12 }}>
                    <input
                      type="checkbox"
                      checked={saveToken}
                      onChange={(e) => setSaveToken(e.target.checked)}
                    />{' '}
                    {t(locale, 'workshop.githubSaveToken')}
                  </label>
                </>
              )}
              {installError && (
                <div style={{ color: 'var(--danger)', fontSize: 12 }}>{installError}</div>
              )}
              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={inspecting || !repoUrl.trim()}
                  onClick={() => void runInspect(false)}
                >
                  {t(locale, 'workshop.githubManual')}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={inspecting || !repoUrl.trim()}
                  onClick={() => void runInspect(true)}
                >
                  {inspecting ? '…' : t(locale, 'workshop.githubOneClick')}
                </button>
              </div>
            </div>
          )}

          {wizardStep === 'manual' && inspect && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10, maxHeight: 480, overflow: 'auto' }}>
              {inspect.blockers.length > 0 && (
                <div style={{ color: 'var(--danger)', fontSize: 12 }}>
                  <strong>{t(locale, 'workshop.githubBlockers')}</strong>
                  <ul>
                    {inspect.blockers.map((b) => (
                      <li key={b}>{b}</li>
                    ))}
                  </ul>
                </div>
              )}
              {inspect.warnings.length > 0 && (
                <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>
                  <strong>{t(locale, 'workshop.githubWarnings')}</strong>
                  <ul>
                    {inspect.warnings.map((w) => (
                      <li key={w}>{w}</li>
                    ))}
                  </ul>
                </div>
              )}
              <label style={{ fontSize: 12 }}>
                {t(locale, 'workshop.githubSelectTag')}
                <select
                  value={selectedTag}
                  onChange={(e) => setSelectedTag(e.target.value)}
                  style={inputStyle}
                >
                  {(inspect.availableTags.length
                    ? inspect.availableTags
                    : [{ tag: inspect.releaseTag, releaseId: 0, isPrerelease: inspect.isPrerelease }]
                  ).map((tg) => (
                    <option key={tg.tag} value={tg.tag}>
                      {tg.tag}
                      {tg.isPrerelease ? ' (pre)' : ''}
                    </option>
                  ))}
                </select>
              </label>
              <label style={{ fontSize: 12 }}>
                {t(locale, 'workshop.githubSelectCandidate')}
                <select
                  value={selectedCandidate?.id || ''}
                  onChange={(e) => {
                    const c = inspect.candidates.find((x) => x.id === e.target.value) || null;
                    setSelectedCandidate(c);
                    if (c) {
                      setHostPort(c.suggestedHostPort ? String(c.suggestedHostPort) : '');
                      setOpenPath(c.openPath || '/');
                      setHealthPath(c.healthPath || '');
                      setService(c.service || '');
                    }
                  }}
                  style={inputStyle}
                >
                  {inspect.candidates.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.title} ({c.runtime}, conf {c.confidence})
                    </option>
                  ))}
                </select>
              </label>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
                <label style={{ fontSize: 12 }}>
                  {t(locale, 'workshop.githubHostPort')}
                  <input value={hostPort} onChange={(e) => setHostPort(e.target.value)} style={inputStyle} />
                </label>
                <label style={{ fontSize: 12 }}>
                  {t(locale, 'workshop.githubService')}
                  <input value={service} onChange={(e) => setService(e.target.value)} style={inputStyle} />
                </label>
                <label style={{ fontSize: 12 }}>
                  {t(locale, 'workshop.githubOpenPath')}
                  <input value={openPath} onChange={(e) => setOpenPath(e.target.value)} style={inputStyle} />
                </label>
                <label style={{ fontSize: 12 }}>
                  {t(locale, 'workshop.githubHealthPath')}
                  <input value={healthPath} onChange={(e) => setHealthPath(e.target.value)} style={inputStyle} />
                </label>
              </div>
              {selectedCandidate && selectedCandidate.envRequirements.length > 0 && (
                <div>
                  <div style={{ fontSize: 12, fontWeight: 600 }}>{t(locale, 'workshop.githubEnvTitle')}</div>
                  {selectedCandidate.envRequirements.map((e) => (
                    <label key={e.key} style={{ fontSize: 12, display: 'block', marginTop: 6 }}>
                      {e.key}
                      {e.required ? ' *' : ''}
                      <input
                        type={e.secret ? 'password' : 'text'}
                        value={envValues[e.key] || ''}
                        onChange={(ev) =>
                          setEnvValues((prev) => ({ ...prev, [e.key]: ev.target.value }))
                        }
                        style={inputStyle}
                      />
                    </label>
                  ))}
                </div>
              )}
              {selectedCandidate && selectedCandidate.riskSummary.length > 0 && (
                <div style={{ fontSize: 12 }}>
                  <strong>{t(locale, 'workshop.githubRiskTitle')}</strong>
                  <ul>
                    {selectedCandidate.riskSummary.map((r) => (
                      <li key={r}>{r}</li>
                    ))}
                  </ul>
                  <label>
                    <input
                      type="checkbox"
                      checked={confirmBinds}
                      onChange={(e) => setConfirmBinds(e.target.checked)}
                    />{' '}
                    confirm bind mounts
                  </label>
                </div>
              )}
              {installError && (
                <div style={{ color: 'var(--danger)', fontSize: 12 }}>{installError}</div>
              )}
              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
                <button type="button" className="btn btn-secondary" onClick={() => setWizardStep('url')}>
                  {t(locale, 'common.back')}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={!selectedCandidate || inspect.blockers.length > 0}
                  onClick={() => {
                    if (inspect && selectedCandidate) void runInstall(inspect, selectedCandidate, false);
                  }}
                >
                  {t(locale, 'workshop.githubInstall')}
                </button>
              </div>
            </div>
          )}

          {wizardStep === 'installing' && (
            <div style={{ fontSize: 13 }}>
              <div style={{ marginBottom: 8 }}>{progress ? stageLabel(progress.stage) : '…'}</div>
              <div style={{ color: 'var(--text-secondary)', fontSize: 12 }}>
                {progress?.message}
              </div>
            </div>
          )}
        </Modal>
      )}
    </motion.div>
  );
}

const inputStyle: React.CSSProperties = {
  display: 'block',
  width: '100%',
  marginTop: 4,
  padding: '8px 10px',
  borderRadius: 6,
  border: '1px solid var(--border)',
  background: 'var(--bg-2)',
  color: 'var(--text)',
  fontSize: 12,
};
