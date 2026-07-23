'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { motion, useReducedMotion } from 'framer-motion';
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Code2,
  ExternalLink,
  Github,
  HelpCircle,
  Layers,
  Loader,
  Package,
  Pause,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  ScrollText,
  Terminal,
  Trash2,
  X,
  XCircle,
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

function renderStatusDot(state: CreativeAppSummary['state']) {
  switch (state) {
    case 'running':
      return <span className="h-2 w-2 rounded-full bg-emerald-500 animate-pulse shrink-0" />;
    case 'starting':
    case 'installing':
    case 'stopping':
    case 'deleting':
      return <RefreshCw size={12} className="animate-spin text-[var(--primary)] shrink-0" />;
    case 'installed_stopped':
    case 'available':
      return <span className="h-2 w-2 rounded-full bg-zinc-400 dark:bg-zinc-500 shrink-0" />;
    case 'disabled':
      return <span className="h-2 w-2 rounded-full bg-zinc-300 dark:bg-zinc-600 shrink-0" />;
    case 'install_failed':
    case 'start_failed':
    case 'delete_failed':
    case 'runtime_unavailable':
      return <span className="h-2 w-2 rounded-full bg-rose-500 shrink-0" />;
    default:
      return <span className="h-2 w-2 rounded-full bg-zinc-400 shrink-0" />;
  }
}

export default function WorkshopPage({ onInstall }: WorkshopPageProps) {
  void onInstall;
  const prefersReducedMotion = useReducedMotion();
  const { apps, loading, error, reload, busyIds, withBusy } = useCreativeAppCatalog();
  const [locale, setLocale] = useState<Locale>('zh');
  const [toast, setToast] = useState<string | null>(null);
  const [addMenu, setAddMenu] = useState<AddMenu>('closed');

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

  const [deleteTarget, setDeleteTarget] = useState<CreativeAppSummary | null>(null);
  const [deleteVolumes, setDeleteVolumes] = useState(false);
  const [deleteImages, setDeleteImages] = useState(false);

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

  const [browserApp, setBrowserApp] = useState<CreativeAppSummary | null>(null);
  const [browserUrl, setBrowserUrl] = useState('');
  const browserHostRef = useRef<HTMLDivElement | null>(null);

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
      setBrowserApp(app);
      setBrowserUrl(target.url);
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
    setInstallError(null);
    if (!repoUrl.trim()) return;
    setInspecting(true);
    try {
      const api = window.nativesAPI?.creativeApp;
      const res = await api?.inspectGithub?.({
        repositoryUrl: repoUrl.trim(),
        token: tokenForRequest(),
        saveToken: tokenMode === 'once' ? saveToken : false,
        oneClick,
        releaseTag: oneClick ? null : selectedTag || null,
      });
      if (!res) throw new Error('Inspect failed');
      setInspect(res);
      const tag = res.availableTags[0]?.tag || res.releaseTag || '';
      setSelectedTag(tag);
      const cand = res.candidates[0] || null;
      setSelectedCandidate(cand);
      if (cand) {
        setHostPort(cand.suggestedHostPort ? String(cand.suggestedHostPort) : '');
        setOpenPath(cand.openPath || '/');
        setHealthPath(cand.healthPath || '');
        setService(cand.service || '');
        const env: Record<string, string> = {};
        for (const e of cand.envRequirements) env[e.key] = '';
        setEnvValues(env);
      }
      if (oneClick && res.candidates.length > 0 && res.blockers.length === 0 && res.candidates[0]) {
        await runInstall(res, res.candidates[0], true);
      } else {
        setWizardStep('manual');
      }
    } catch (err) {
      setInstallError(classifyError(err).userMessage);
    } finally {
      setInspecting(false);
    }
  };

  const runInstall = async (
    inspectResult: CreativeAppInspectResult,
    cand: CreativeAppInstallCandidate,
    oneClick: boolean,
  ) => {
    setInstallError(null);
    setWizardStep('installing');
    try {
      const api = window.nativesAPI?.creativeApp;
      await api?.installGithub?.({
        repositoryUrl: inspectResult.repositoryUrl,
        releaseTag: selectedTag || inspectResult.releaseTag,
        releaseId: inspectResult.releaseId,
        candidateId: cand.id,
        token: tokenForRequest(),
        hostPort: hostPort ? Number(hostPort) : cand.suggestedHostPort,
        openPath: openPath || cand.openPath,
        healthPath: healthPath || cand.healthPath,
        service: service || cand.service,
        env: Object.entries(envValues).map(([key, value]) => ({ key, value })),
        confirmBindMounts: confirmBinds || oneClick,
      });
      showToast(t(locale, 'workshop.githubInstallSuccess'));
      setWizardOpen(false);
      resetWizard();
      await reload();
    } catch (err) {
      setInstallError(classifyError(err).userMessage);
      setWizardStep(oneClick ? 'url' : 'manual');
    }
  };

  const stageLabel = (stage: CreativeAppProgressEvent['stage']) => {
    const map: Record<string, string> = {
      download: 'workshop.githubStageDownload',
      extract: 'workshop.githubStageExtract',
      prepare: 'workshop.githubStagePrepare',
      compose: 'workshop.githubStageCompose',
      build: 'workshop.githubStageBuild',
      start: 'workshop.githubStageStart',
      health: 'workshop.githubStageHealth',
    };
    return t(locale, map[stage] || 'workshop.githubStageInstall');
  };

  if (browserApp) {
    return (
      <div className="flex flex-col h-full bg-[var(--background)]">
        <div className="flex items-center gap-2 px-4 py-2.5 border-b border-[var(--border)] bg-[var(--surface)] shrink-0">
          <button
            type="button"
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all"
            onClick={() => void window.nativesAPI?.creativeApp?.browserBack?.()}
            title={t(locale, 'workshop.browserBack')}
          >
            <ChevronLeft size={14} />
          </button>
          <button
            type="button"
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all"
            onClick={() => void window.nativesAPI?.creativeApp?.browserForward?.()}
            title={t(locale, 'workshop.browserForward')}
          >
            <ChevronRight size={14} />
          </button>
          <button
            type="button"
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-all"
            onClick={() => void window.nativesAPI?.creativeApp?.browserReload?.()}
            title={t(locale, 'workshop.browserReload')}
          >
            <RefreshCw size={14} />
          </button>
          <div
            className="flex-1 text-xs font-mono text-[var(--text-secondary)] px-3 py-1.5 border border-[var(--border)] rounded-lg bg-[var(--surface-subtle)] truncate"
            title={browserUrl}
          >
            {browserUrl || t(locale, 'workshop.browserAddress')}
          </div>
          <button
            type="button"
            className="flex h-8 items-center gap-1.5 px-3 rounded-lg border border-[var(--border)] bg-[var(--surface)] text-xs font-medium text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
            onClick={() => void closeBrowser()}
          >
            <X size={14} />
            {t(locale, 'workshop.browserBackToList')}
          </button>
        </div>
        <div ref={browserHostRef} className="flex-1 min-h-0 bg-[var(--background)]" />
      </div>
    );
  }

  return (
    <motion.div
      initial={prefersReducedMotion ? undefined : { opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={prefersReducedMotion ? undefined : { type: 'spring', stiffness: 60, damping: 16, mass: 1 }}
      className="flex flex-col h-full overflow-y-auto"
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={handleDrop}
    >
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
            onClick={() => void reload()}
            title={t(locale, 'common.refresh')}
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
          </button>

          <button
            type="button"
            className="flex h-8 items-center gap-1.5 px-3 rounded-lg bg-[var(--primary)] text-white text-xs font-medium hover:opacity-90 active:scale-95 transition-all shadow-sm"
            onClick={() => setAddMenu((m) => (m === 'open' ? 'closed' : 'open'))}
          >
            <Plus size={14} />
            <span>{t(locale, 'workshop.add')}</span>
          </button>

          {addMenu === 'open' && (
            <>
              <div
                className="fixed inset-0 z-10"
                onClick={() => setAddMenu('closed')}
              />
              <div className="absolute right-0 top-full mt-1.5 w-52 bg-[var(--surface)] border border-[var(--border)] rounded-xl shadow-lg p-1 z-20 flex flex-col gap-0.5">
                <button
                  type="button"
                  className="flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all w-full text-left"
                  onClick={() => {
                    setAddMenu('closed');
                    setShowCreateDialog(true);
                  }}
                >
                  <Layers size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuCreate')}</span>
                </button>
                <label className="flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all w-full cursor-pointer">
                  <Package size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuImport')}</span>
                  <input
                    type="file"
                    accept=".zip"
                    className="hidden"
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
                  className="flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all w-full text-left"
                  onClick={() => {
                    setAddMenu('closed');
                    resetWizard();
                    setWizardOpen(true);
                  }}
                >
                  <Github size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuGithub')}</span>
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      {dragOver && (
        <div className="m-6 p-8 border-2 border-dashed border-[var(--primary)] rounded-xl text-center text-xs font-semibold text-[var(--primary)] bg-[var(--primary-soft)] animate-pulse">
          {t(locale, 'workshop.releaseToInstall')}
        </div>
      )}

      <div className="p-6 flex-1">
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

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          {apps.map((app) => {
            const busy = busyIds.has(app.id) || isActionBusy(app.state);
            const actions = mergeActionsWithBusy(app.actions, busy);
            const badge = sourceBadge(app.source);
            return (
              <div
                key={app.id}
                className="bg-[var(--surface)] border border-[var(--border)] rounded-xl p-4 flex flex-col justify-between transition-all hover:border-[var(--border-hover)] hover:shadow-sm"
              >
                <div>
                  <div className="flex items-center justify-between gap-2 border-b border-[var(--border-subtle)] pb-2.5 mb-2.5">
                    <div className="font-semibold text-sm text-[var(--text)] truncate" title={app.title}>
                      {app.title}
                    </div>
                    <span
                      className={`text-[10px] font-medium px-2 py-0.5 rounded-full flex items-center gap-1 border shrink-0 ${
                        badge === 'github'
                          ? 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20'
                          : 'bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20'
                      }`}
                    >
                      {badge === 'github' ? <Github size={10} /> : <Code2 size={10} />}
                      <span>
                        {badge === 'github'
                          ? t(locale, 'workshop.sourceGithub')
                          : t(locale, 'workshop.sourceInternal')}
                      </span>
                    </span>
                  </div>

                  <div className="flex items-center gap-2 text-xs text-[var(--text-secondary)] mb-2">
                    <div className="flex items-center gap-1.5 font-medium">
                      {renderStatusDot(app.state)}
                      <span className="text-[var(--text)]">{stateLabel(locale, app.state)}</span>
                    </div>
                    <span className="text-[var(--border)]">•</span>
                    <span className="truncate">{runtimeLabel(locale, app.runtime)}</span>
                    {app.version && (
                      <>
                        <span className="text-[var(--border)]">•</span>
                        <span className="font-mono text-[11px]">{app.version}</span>
                      </>
                    )}
                  </div>

                  {app.lastError && (
                    <div className="text-[11px] text-rose-500 bg-rose-500/10 border border-rose-500/20 p-2 rounded-lg mb-3 flex items-start gap-1.5">
                      <AlertTriangle size={12} className="shrink-0 mt-0.5" />
                      <span className="line-clamp-2">{app.lastError}</span>
                    </div>
                  )}
                </div>

                <div className="flex items-center gap-1.5 mt-3 pt-3 border-t border-[var(--border-subtle)] flex-wrap">
                  {actions.canOpen && (
                    <button
                      type="button"
                      className="h-8 px-3 text-xs font-medium rounded-lg bg-[var(--primary)] text-white hover:opacity-90 active:scale-95 transition-all flex items-center justify-center gap-1.5 shrink-0"
                      onClick={() => void handleOpen(app)}
                    >
                      <ExternalLink size={13} />
                      <span>{t(locale, 'workshop.actionOpen')}</span>
                    </button>
                  )}
                  {actions.canStart && (
                    <button
                      type="button"
                      className="h-8 px-3 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all flex items-center justify-center gap-1.5 shrink-0"
                      onClick={() => void handleStart(app)}
                    >
                      <Play size={13} />
                      <span>{t(locale, 'workshop.actionStart')}</span>
                    </button>
                  )}
                  {actions.canStop && (
                    <button
                      type="button"
                      className="h-8 px-3 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all flex items-center justify-center gap-1.5 shrink-0"
                      onClick={() => void handleStop(app)}
                    >
                      <Pause size={13} />
                      <span>{t(locale, 'workshop.actionStop')}</span>
                    </button>
                  )}
                  {actions.canRetry && (
                    <button
                      type="button"
                      className="h-8 px-3 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all flex items-center justify-center gap-1.5 shrink-0"
                      onClick={() => void handleStart(app)}
                    >
                      <RotateCcw size={13} />
                      <span>{t(locale, 'workshop.actionRetry')}</span>
                    </button>
                  )}
                  {app.source === 'external_github' && (
                    <button
                      type="button"
                      className="h-8 px-2.5 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text-secondary)] hover:text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all flex items-center justify-center gap-1.5 shrink-0"
                      onClick={() => void openLogs(app)}
                      title={t(locale, 'workshop.actionLogs')}
                    >
                      <ScrollText size={13} />
                    </button>
                  )}
                  {actions.canDelete && (
                    <button
                      type="button"
                      className="h-8 px-2.5 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-rose-500 hover:bg-rose-500/10 hover:border-rose-500/20 transition-all flex items-center justify-center gap-1.5 shrink-0 ml-auto"
                      onClick={() => {
                        setDeleteTarget(app);
                        const d = defaultDeleteOptions();
                        setDeleteVolumes(d.removeVolumes);
                        setDeleteImages(d.removeImages);
                      }}
                      title={t(locale, 'workshop.actionDelete')}
                    >
                      <Trash2 size={13} />
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {toast && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 bg-[var(--text)] text-[var(--bg)] px-4 py-2 rounded-lg text-xs font-medium shadow-lg z-50 animate-fade-in">
          {toast}
        </div>
      )}

      {showCreateDialog && (
        <Modal
          isOpen
          onClose={() => setShowCreateDialog(false)}
          title={t(locale, 'workshop.createModule')}
          width={420}
        >
          <div className="flex flex-col gap-4 py-2">
            <div>
              <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                {t(locale, 'workshop.templateName')}
              </label>
              <input
                value={templateName}
                onChange={(e) => setTemplateName(e.target.value)}
                placeholder={t(locale, 'workshop.templateNamePlaceholder')}
                className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                {t(locale, 'workshop.templateId')}
              </label>
              <input
                value={templateId}
                onChange={(e) => setTemplateId(e.target.value)}
                placeholder={t(locale, 'workshop.templateIdPlaceholder')}
                className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
              />
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
                onClick={() => setShowCreateDialog(false)}
              >
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-white hover:opacity-90 transition-all flex items-center gap-1.5"
                disabled={creating}
                onClick={() => void createTemplate()}
              >
                {creating ? <Loader size={14} className="animate-spin" /> : null}
                <span>{creating ? t(locale, 'workshop.creating') : t(locale, 'workshop.createTemplate')}</span>
              </button>
            </div>
          </div>
        </Modal>
      )}

      {permDialog && (
        <Modal
          isOpen
          onClose={() => setPermDialog(null)}
          title={t(locale, 'workshop.permissionTitle')}
          width={440}
        >
          <div className="flex flex-col gap-3 py-1">
            <p className="text-xs text-[var(--text-secondary)]">
              {t(locale, 'workshop.permissionDesc').replace('{name}', permDialog.moduleName)}
            </p>
            <div className="bg-[var(--surface-subtle)] p-3 rounded-lg border border-[var(--border)] max-h-48 overflow-y-auto">
              <ul className="space-y-2 text-xs text-[var(--text)]">
                {permDialog.permissions.map((p) => (
                  <li key={p} className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      id={`perm-${p}`}
                      checked={selectedPerms.has(p)}
                      onChange={(e) => {
                        setSelectedPerms((prev) => {
                          const n = new Set(prev);
                          if (e.target.checked) n.add(p);
                          else n.delete(p);
                          return n;
                        });
                      }}
                      className="rounded border-[var(--border)] text-[var(--primary)] focus:ring-0"
                    />
                    <label htmlFor={`perm-${p}`} className="cursor-pointer font-mono text-[11px]">
                      {p}
                    </label>
                  </li>
                ))}
              </ul>
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
                onClick={() => setPermDialog(null)}
              >
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-white hover:opacity-90 transition-all flex items-center gap-1.5"
                disabled={installing}
                onClick={() => void confirmImport()}
              >
                {installing ? <Loader size={14} className="animate-spin" /> : null}
                <span>{t(locale, 'workshop.permissionAllowAll')}</span>
              </button>
            </div>
          </div>
        </Modal>
      )}

      {deleteTarget && (
        <Modal
          isOpen
          onClose={() => setDeleteTarget(null)}
          title={t(locale, 'workshop.deleteTitle')}
          width={420}
        >
          <div className="flex flex-col gap-3 py-1">
            <p className="text-xs text-[var(--text-secondary)]">
              {t(locale, 'workshop.deleteDesc')}
            </p>
            <div className="p-3 bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg font-semibold text-sm text-[var(--text)]">
              {deleteTarget.title}
            </div>

            {deleteTarget.source === 'external_github' && (
              <div className="flex flex-col gap-2 pt-1">
                <label className="flex items-center gap-2 text-xs text-[var(--text)] cursor-pointer">
                  <input
                    type="checkbox"
                    checked={deleteVolumes}
                    onChange={(e) => setDeleteVolumes(e.target.checked)}
                    className="rounded border-[var(--border)]"
                  />
                  <span>{t(locale, 'workshop.deleteRemoveVolumes')}</span>
                </label>
                <label className="flex items-center gap-2 text-xs text-[var(--text)] cursor-pointer">
                  <input
                    type="checkbox"
                    checked={deleteImages}
                    onChange={(e) => setDeleteImages(e.target.checked)}
                    className="rounded border-[var(--border)]"
                  />
                  <span>{t(locale, 'workshop.deleteRemoveImages')}</span>
                </label>
              </div>
            )}

            <div className="flex justify-end gap-2 pt-3">
              <button
                type="button"
                className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
                onClick={() => setDeleteTarget(null)}
              >
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="h-9 px-4 text-xs font-medium rounded-lg bg-rose-500 text-white hover:bg-rose-600 transition-all flex items-center gap-1.5"
                onClick={() => void doDelete()}
              >
                <Trash2 size={14} />
                <span>{t(locale, 'workshop.deleteConfirm')}</span>
              </button>
            </div>
          </div>
        </Modal>
      )}

      {logsFor && (
        <Modal isOpen onClose={() => setLogsFor(null)} title={t(locale, 'workshop.logsTitle')} width={640}>
          <div className="py-1">
            <pre className="max-h-96 overflow-auto text-[11px] font-mono bg-zinc-950 text-zinc-200 p-4 rounded-xl border border-zinc-800 leading-relaxed">
              {logsText}
            </pre>
          </div>
        </Modal>
      )}

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
            <div className="flex flex-col gap-4 py-1">
              <div>
                <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                  {t(locale, 'workshop.githubRepoUrl')}
                </label>
                <input
                  value={repoUrl}
                  onChange={(e) => setRepoUrl(e.target.value)}
                  placeholder={t(locale, 'workshop.githubRepoPlaceholder')}
                  className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                />
              </div>

              <div>
                <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                  {t(locale, 'settings.githubToken')}
                </label>
                <div className="grid grid-cols-3 gap-1.5 p-1 bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg">
                  {(['public', 'saved', 'once'] as const).map((m) => (
                    <button
                      key={m}
                      type="button"
                      onClick={() => setTokenMode(m)}
                      className={`h-7 text-[11px] font-medium rounded-md transition-all ${
                        tokenMode === m
                          ? 'bg-[var(--surface)] text-[var(--primary)] shadow-sm font-semibold'
                          : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
                      }`}
                    >
                      {m === 'public'
                        ? t(locale, 'workshop.githubPublicAccess')
                        : m === 'saved'
                          ? t(locale, 'workshop.githubUseSavedToken')
                          : t(locale, 'workshop.githubOneShotToken')}
                    </button>
                  ))}
                </div>
              </div>

              {tokenMode === 'once' && (
                <div className="space-y-2">
                  <input
                    type="password"
                    value={tokenInput}
                    onChange={(e) => setTokenInput(e.target.value)}
                    placeholder={t(locale, 'workshop.githubTokenPlaceholder')}
                    className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                  />
                  <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer">
                    <input
                      type="checkbox"
                      checked={saveToken}
                      onChange={(e) => setSaveToken(e.target.checked)}
                      className="rounded border-[var(--border)]"
                    />
                    <span>{t(locale, 'workshop.githubSaveToken')}</span>
                  </label>
                </div>
              )}

              {installError && (
                <div className="text-xs text-rose-500 bg-rose-500/10 border border-rose-500/20 p-2.5 rounded-lg flex items-start gap-2">
                  <AlertTriangle size={14} className="shrink-0 mt-0.5" />
                  <span>{installError}</span>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2">
                <button
                  type="button"
                  className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
                  disabled={inspecting || !repoUrl.trim()}
                  onClick={() => void runInspect(false)}
                >
                  {t(locale, 'workshop.githubManual')}
                </button>
                <button
                  type="button"
                  className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-white hover:opacity-90 transition-all flex items-center gap-1.5"
                  disabled={inspecting || !repoUrl.trim()}
                  onClick={() => void runInspect(true)}
                >
                  {inspecting ? <Loader size={14} className="animate-spin" /> : null}
                  <span>{inspecting ? '…' : t(locale, 'workshop.githubOneClick')}</span>
                </button>
              </div>
            </div>
          )}

          {wizardStep === 'manual' && inspect && (
            <div className="flex flex-col gap-3 py-1 max-h-[460px] overflow-y-auto pr-1">
              {inspect.blockers.length > 0 && (
                <div className="text-xs text-rose-500 bg-rose-500/10 border border-rose-500/20 p-3 rounded-lg">
                  <strong className="block mb-1 font-semibold">{t(locale, 'workshop.githubBlockers')}</strong>
                  <ul className="list-disc list-inside space-y-1">
                    {inspect.blockers.map((b) => (
                      <li key={b}>{b}</li>
                    ))}
                  </ul>
                </div>
              )}

              {inspect.warnings.length > 0 && (
                <div className="text-xs text-amber-600 dark:text-amber-400 bg-amber-500/10 border border-amber-500/20 p-3 rounded-lg">
                  <strong className="block mb-1 font-semibold">{t(locale, 'workshop.githubWarnings')}</strong>
                  <ul className="list-disc list-inside space-y-1">
                    {inspect.warnings.map((w) => (
                      <li key={w}>{w}</li>
                    ))}
                  </ul>
                </div>
              )}

              <div>
                <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
                  {t(locale, 'workshop.githubSelectTag')}
                </label>
                <select
                  value={selectedTag}
                  onChange={(e) => setSelectedTag(e.target.value)}
                  className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
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
              </div>

              <div>
                <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
                  {t(locale, 'workshop.githubSelectCandidate')}
                </label>
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
                  className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                >
                  {inspect.candidates.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.title} ({c.runtime}, conf {c.confidence})
                    </option>
                  ))}
                </select>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
                    {t(locale, 'workshop.githubHostPort')}
                  </label>
                  <input
                    value={hostPort}
                    onChange={(e) => setHostPort(e.target.value)}
                    className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
                    {t(locale, 'workshop.githubService')}
                  </label>
                  <input
                    value={service}
                    onChange={(e) => setService(e.target.value)}
                    className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
                    {t(locale, 'workshop.githubOpenPath')}
                  </label>
                  <input
                    value={openPath}
                    onChange={(e) => setOpenPath(e.target.value)}
                    className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
                    {t(locale, 'workshop.githubHealthPath')}
                  </label>
                  <input
                    value={healthPath}
                    onChange={(e) => setHealthPath(e.target.value)}
                    className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                  />
                </div>
              </div>

              {selectedCandidate && selectedCandidate.envRequirements.length > 0 && (
                <div className="space-y-2 pt-1">
                  <div className="text-xs font-semibold text-[var(--text)]">{t(locale, 'workshop.githubEnvTitle')}</div>
                  <div className="space-y-2">
                    {selectedCandidate.envRequirements.map((e) => (
                      <div key={e.key}>
                        <label className="block text-[11px] font-medium text-[var(--text-secondary)] mb-1">
                          {e.key} {e.required ? <span className="text-rose-500">*</span> : null}
                        </label>
                        <input
                          type={e.secret ? 'password' : 'text'}
                          value={envValues[e.key] || ''}
                          onChange={(ev) =>
                            setEnvValues((prev) => ({ ...prev, [e.key]: ev.target.value }))
                          }
                          className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
                        />
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {selectedCandidate && selectedCandidate.riskSummary.length > 0 && (
                <div className="text-xs bg-[var(--surface-subtle)] p-3 rounded-lg border border-[var(--border)] space-y-2">
                  <strong className="block font-semibold text-[var(--text)]">{t(locale, 'workshop.githubRiskTitle')}</strong>
                  <ul className="list-disc list-inside text-[var(--text-secondary)] space-y-1">
                    {selectedCandidate.riskSummary.map((r) => (
                      <li key={r}>{r}</li>
                    ))}
                  </ul>
                  <label className="flex items-center gap-2 pt-1 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={confirmBinds}
                      onChange={(e) => setConfirmBinds(e.target.checked)}
                      className="rounded border-[var(--border)]"
                    />
                    <span className="font-medium text-[var(--text)]">confirm bind mounts</span>
                  </label>
                </div>
              )}

              {installError && (
                <div className="text-xs text-rose-500 bg-rose-500/10 border border-rose-500/20 p-2.5 rounded-lg flex items-start gap-2">
                  <AlertTriangle size={14} className="shrink-0 mt-0.5" />
                  <span>{installError}</span>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2">
                <button
                  type="button"
                  className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
                  onClick={() => setWizardStep('url')}
                >
                  {t(locale, 'common.back')}
                </button>
                <button
                  type="button"
                  className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-white hover:opacity-90 transition-all flex items-center gap-1.5"
                  disabled={!selectedCandidate || inspect.blockers.length > 0}
                  onClick={() => {
                    if (inspect && selectedCandidate) void runInstall(inspect, selectedCandidate, false);
                  }}
                >
                  <span>{t(locale, 'workshop.githubInstall')}</span>
                </button>
              </div>
            </div>
          )}

          {wizardStep === 'installing' && (
            <div className="py-6 flex flex-col items-center justify-center text-center space-y-3">
              <Loader size={28} className="animate-spin text-[var(--primary)]" />
              <div className="text-sm font-semibold text-[var(--text)]">
                {progress ? stageLabel(progress.stage) : '…'}
              </div>
              <div className="text-xs text-[var(--text-secondary)] max-w-sm">
                {progress?.message}
              </div>
            </div>
          )}
        </Modal>
      )}
    </motion.div>
  );
}
