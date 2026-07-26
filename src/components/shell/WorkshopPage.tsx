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
  Folder,
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
import { t, useLocale, type Locale } from '@/i18n';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState, LoadingState } from '@/components/ui/EmptyState';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import { useCreativeAppCatalog } from '@/hooks/useCreativeAppCatalog';
import CreativeHome from '@/components/creative/CreativeHome';
import {
  defaultDeleteOptions,
  deleteNeedsDockerOptions,
  isActionBusy,
  mergeActionsWithBusy,
  shouldAutoOpenAfterStart,
  sourceBadge,
} from '@/lib/creative-app';
import type {
  CreativeAppBrowserBounds,
  CreativeAppInspectResult,
  CreativeAppInstallCandidate,
  CreativeAppProgressEvent,
  CreativeAppSummary,
  LaunchPlan,
  LocalProjectScanResult,
  PackageManager,
} from '@/lib/tauri-adapter';
import {
  buildCreateRequest,
  canProceedFromScan,
  defaultLocalTitleFromPath,
  deleteLocalConfirmNote,
  localIssueLabel,
  pickPackageManager,
  planSummaryLines,
  type LocalWizardStep,
} from '@/lib/local-creative';

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
  if (runtime === 'local_static') return t(locale, 'workshop.runtimeLocalStatic');
  if (runtime === 'node_dev_server') return t(locale, 'workshop.runtimeNodeDev');
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
  // reactive useLocale：此前一发式 getLocale 导致切换语言后 creative 全面（经 prop 下发）停留旧语言
  const locale = useLocale();
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
  const [logAutoScroll, setLogAutoScroll] = useState(true);
  const [logFilter, setLogFilter] = useState('');
  const logPreRef = useRef<HTMLPreElement | null>(null);

  // Local project wizard (four steps)
  const [localWizardOpen, setLocalWizardOpen] = useState(false);
  const [localStep, setLocalStep] = useState<LocalWizardStep>('basic');
  const [localRoot, setLocalRoot] = useState('');
  const [localTitle, setLocalTitle] = useState('');
  const [localDesc, setLocalDesc] = useState('');
  const [localScanning, setLocalScanning] = useState(false);
  const [localScan, setLocalScan] = useState<LocalProjectScanResult | null>(null);
  const [localScanError, setLocalScanError] = useState<string | null>(null);
  const [localLaunchMode, setLocalLaunchMode] = useState<'smart' | 'custom'>('smart');
  const [localPlan, setLocalPlan] = useState<LaunchPlan | null>(null);
  const [localPm, setLocalPm] = useState<PackageManager | undefined>(undefined);
  const [localScript, setLocalScript] = useState('');
  const [localAutoOpen, setLocalAutoOpen] = useState(true);
  const [localStartAfterSave, setLocalStartAfterSave] = useState(false);
  const [localSaving, setLocalSaving] = useState(false);
  const [localCwd, setLocalCwd] = useState('.');
  const [localOpenPath, setLocalOpenPath] = useState('/');
  const [localPortMode, setLocalPortMode] = useState<'auto' | 'fixed'>('auto');
  const [localPortValue, setLocalPortValue] = useState('');
  const [localAiPreview, setLocalAiPreview] = useState<string | null>(null);
  const [localAiBusy, setLocalAiBusy] = useState(false);
  const [localAiPendingConfirm, setLocalAiPendingConfirm] = useState(false);

  const [editLocal, setEditLocal] = useState<CreativeAppSummary | null>(null);
  const [editTitle, setEditTitle] = useState('');
  const [editAutoOpen, setEditAutoOpen] = useState(true);
  const [editSaving, setEditSaving] = useState(false);
  const [editPlan, setEditPlan] = useState<LaunchPlan | null>(null);
  const [editEnvKeys, setEditEnvKeys] = useState<string[]>([]);
  const [editPortMode, setEditPortMode] = useState<'auto' | 'fixed'>('auto');
  const [editPortValue, setEditPortValue] = useState('');
  const [editScript, setEditScript] = useState('');
  const [editOpenPath, setEditOpenPath] = useState('/');
  const [editCwd, setEditCwd] = useState('.');
  const [editLoading, setEditLoading] = useState(false);

  const [depInstallFor, setDepInstallFor] = useState<CreativeAppSummary | null>(null);
  const [depConfirmChecked, setDepConfirmChecked] = useState(false);
  const [depInstalling, setDepInstalling] = useState(false);
  const [depCommand, setDepCommand] = useState<string>('');

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2400);
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
    // Open surface still differs by source (iframe vs child webview), but
    // resolution goes through creativeApp.getOpenTarget for non-internal paths
    // and openInternalModule for workshop modules (Bridge stays internal-only).
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
        // Unified lifecycle: all three sources start via creativeApp.start
        // (adapters map internal → enable_module).
        const updated = await window.nativesAPI?.creativeApp?.start?.(app.id);
        if (
          updated?.state === 'running' &&
          shouldAutoOpenAfterStart(updated) &&
          updated.source !== 'internal'
        ) {
          await openExternal(updated);
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
        await window.nativesAPI?.creativeApp?.stop?.(app.id);
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
        // Unified delete: adapters keep Docker options meaningful only for
        // external_github; local never touches project files; internal uninstalls module.
        await window.nativesAPI?.creativeApp?.delete?.(id, {
          removeVolumes: deleteVolumes,
          removeImages: deleteImages,
        });
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
    setLogAutoScroll(true);
    try {
      if (app.source === 'local_project' && window.nativesAPI?.creativeApp?.getLocalLogs) {
        const lines = await window.nativesAPI.creativeApp.getLocalLogs(app.id, 400);
        if (Array.isArray(lines) && lines.length > 0) {
          setLogsText(
            lines
              .map((l) => `[${l.stream}] ${l.text}`)
              .join('\n'),
          );
          return;
        }
      }
      const text = await window.nativesAPI?.creativeApp?.logs?.(app.id, 200);
      setLogsText(text || '');
    } catch (err) {
      setLogsText(classifyError(err).userMessage);
    }
  };

  useEffect(() => {
    if (!logsFor) return;
    const api = window.nativesAPI?.creativeApp;
    if (!api?.onLog) return;
    return api.onLog((ev) => {
      if (ev.appId !== logsFor.id) return;
      setLogsText((prev) => {
        const line = `[${ev.stream}] ${ev.text}`;
        if (!prev || prev === '…') return line;
        return `${prev}\n${line}`;
      });
    });
  }, [logsFor]);

  const resetLocalWizard = () => {
    setLocalStep('basic');
    setLocalRoot('');
    setLocalTitle('');
    setLocalDesc('');
    setLocalScanning(false);
    setLocalScan(null);
    setLocalScanError(null);
    setLocalLaunchMode('smart');
    setLocalPlan(null);
    setLocalPm(undefined);
    setLocalScript('');
    setLocalAutoOpen(true);
    setLocalStartAfterSave(false);
    setLocalSaving(false);
    setLocalCwd('.');
    setLocalOpenPath('/');
    setLocalPortMode('auto');
    setLocalPortValue('');
    setLocalAiPreview(null);
    setLocalAiBusy(false);
    setLocalAiPendingConfirm(false);
  };

  const pickLocalFolder = async () => {
    try {
      const path = await window.nativesAPI?.dialog?.pickDirectory?.();
      if (!path) return;
      setLocalRoot(path);
      if (!localTitle.trim()) setLocalTitle(defaultLocalTitleFromPath(path));
    } catch (err) {
      showToast(classifyError(err).userMessage);
    }
  };

  const runLocalScan = async (root: string) => {
    setLocalScanning(true);
    setLocalScanError(null);
    try {
      const scan = await window.nativesAPI?.creativeApp?.inspectLocal?.({ projectRoot: root });
      if (!scan) throw new Error('inspectLocal unavailable');
      setLocalScan(scan);
      const pm = pickPackageManager(scan);
      setLocalPm(pm);
      setLocalScript(scan.preferredScript || scan.scripts[0] || '');
      if (scan.rulePlan) {
        setLocalPlan(scan.rulePlan);
      } else {
        setLocalPlan(null);
      }
      if (scan.existingId) {
        showToast(t(locale, 'workshop.localAlreadyRegistered'));
      }
      setLocalStep('scan');
    } catch (err) {
      setLocalScanError(classifyError(err).userMessage);
    } finally {
      setLocalScanning(false);
    }
  };

  const applyCustomPlanFromScan = (): LaunchPlan | null => {
    if (!localScan) return null;
    const base = localPlan ?? localScan.rulePlan;
    if (!base) {
      // Minimal static fallback when index.html exists via kind
      if (localScan.projectKind === 'html') {
        return {
          schemaVersion: 1,
          source: 'user',
          projectKind: 'html',
          runtime: 'static_http',
          program: 'internal',
          cwdRelative: localCwd.trim() || '.',
          entryFile: 'index.html',
          args: [],
          environmentKeys: [],
          port: {
            mode: localPortMode,
            value:
              localPortMode === 'fixed' && localPortValue
                ? Number(localPortValue)
                : undefined,
          },
          openPath: localOpenPath.trim() || '/',
          healthPath: localOpenPath.trim() || '/',
          startupTimeoutMs: 60000,
          autoOpen: localAutoOpen,
          reason: 'user custom static',
        };
      }
      return null;
    }
    if (base.runtime === 'static_http') {
      return {
        ...base,
        source: 'user',
        cwdRelative: localCwd.trim() || base.cwdRelative || '.',
        openPath: localOpenPath.trim() || base.openPath || '/',
        healthPath: localOpenPath.trim() || base.healthPath || '/',
        autoOpen: localAutoOpen,
        port: {
          mode: localPortMode,
          value:
            localPortMode === 'fixed' && localPortValue
              ? Number(localPortValue)
              : undefined,
        },
      };
    }
    const program = (localPm || base.program) as LaunchPlan['program'];
    return {
      ...base,
      source: 'user',
      program: program === 'internal' ? 'npm' : program,
      script: localScript || base.script,
      cwdRelative: localCwd.trim() || base.cwdRelative || '.',
      openPath: localOpenPath.trim() || base.openPath || '/',
      healthPath: localOpenPath.trim() || base.healthPath || '/',
      autoOpen: localAutoOpen,
      port: {
        mode: localPortMode,
        value:
          localPortMode === 'fixed' && localPortValue
            ? Number(localPortValue)
            : undefined,
      },
    };
  };

  const saveLocalCreative = async (startAfter: boolean) => {
    if (!localRoot.trim()) return;
    setLocalSaving(true);
    try {
      const mode = localLaunchMode;
      const plan =
        mode === 'custom' ? applyCustomPlanFromScan() : localPlan ?? localScan?.rulePlan ?? null;
      if (!plan) {
        showToast(t(locale, 'workshop.localNoPlan'));
        setLocalSaving(false);
        return;
      }
      const req = buildCreateRequest({
        projectRoot: localRoot.trim(),
        title: localTitle,
        description: localDesc,
        launchMode: mode,
        launchPlan: plan,
        autoOpen: localAutoOpen,
        startAfterSave: startAfter,
        packageManager: localPm,
      });
      const created = await window.nativesAPI?.creativeApp?.createLocal?.(req);
      if (!created) throw new Error('createLocal failed');
      if (startAfter) {
        try {
          await window.nativesAPI?.creativeApp?.start?.(created.id);
        } catch (err) {
          showToast(classifyError(err).userMessage);
        }
      }
      showToast(t(locale, 'workshop.localSaved'));
      setLocalWizardOpen(false);
      resetLocalWizard();
      void reload();
    } catch (err) {
      showToast(classifyError(err).userMessage);
    } finally {
      setLocalSaving(false);
    }
  };

  const handleRestart = async (app: CreativeAppSummary) => {
    if (busyIds.has(app.id)) return;
    await withBusy(app.id, async () => {
      try {
        await window.nativesAPI?.creativeApp?.restart?.(app.id);
        void reload();
      } catch (err) {
        showToast(classifyError(err).userMessage);
      }
    });
  };

  const handleResolveOrphan = async (app: CreativeAppSummary, restart: boolean) => {
    if (busyIds.has(app.id)) return;
    await withBusy(app.id, async () => {
      try {
        await window.nativesAPI?.creativeApp?.resolveOrphan?.(app.id, restart);
        void reload();
      } catch (err) {
        showToast(classifyError(err).userMessage);
      }
    });
  };

  const openEditLocal = async (app: CreativeAppSummary) => {
    setEditLocal(app);
    setEditTitle(app.title);
    setEditAutoOpen(Boolean(app.localProject?.autoOpen ?? true));
    setEditLoading(true);
    try {
      const cfg = await window.nativesAPI?.creativeApp?.getLocalConfig?.(app.id);
      if (cfg?.launchPlan) {
        setEditPlan(cfg.launchPlan);
        setEditPortMode(cfg.launchPlan.port.mode);
        setEditPortValue(cfg.launchPlan.port.value != null ? String(cfg.launchPlan.port.value) : '');
        setEditScript(cfg.launchPlan.script || '');
        setEditOpenPath(cfg.launchPlan.openPath || '/');
        setEditCwd(cfg.launchPlan.cwdRelative || '.');
        setEditAutoOpen(cfg.launchPlan.autoOpen);
      }
      setEditEnvKeys(cfg?.envKeys || []);
    } catch (err) {
      showToast(classifyError(err).userMessage);
    } finally {
      setEditLoading(false);
    }
  };

  const saveEditLocal = async () => {
    if (!editLocal) return;
    setEditSaving(true);
    try {
      let launchPlan = editPlan;
      if (launchPlan) {
        launchPlan = {
          ...launchPlan,
          source: 'user',
          cwdRelative: editCwd.trim() || '.',
          script: editScript.trim() || launchPlan.script,
          openPath: editOpenPath.trim() || '/',
          healthPath: editOpenPath.trim() || '/',
          autoOpen: editAutoOpen,
          port: {
            mode: editPortMode,
            value:
              editPortMode === 'fixed' && editPortValue
                ? Number(editPortValue)
                : undefined,
          },
        };
      }
      await window.nativesAPI?.creativeApp?.updateLocal?.({
        id: editLocal.id,
        title: editTitle.trim() || editLocal.title,
        autoOpen: editAutoOpen,
        launchPlan: launchPlan || undefined,
        launchMode: launchPlan ? 'custom' : undefined,
      });
      setEditLocal(null);
      void reload();
    } catch (err) {
      showToast(classifyError(err).userMessage);
    } finally {
      setEditSaving(false);
    }
  };

  useEffect(() => {
    if (!logsFor || !logAutoScroll) return;
    const el = logPreRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [logsText, logsFor, logAutoScroll]);

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
          {browserApp.source !== 'internal' && (
            <>
              <button
                type="button"
                className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                onClick={() => void handleStop(browserApp)}
                title={t(locale, 'workshop.actionStop')}
              >
                <Pause size={14} />
              </button>
              <button
                type="button"
                className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                onClick={() => void handleRestart(browserApp)}
                title={t(locale, 'workshop.actionRestart')}
              >
                <RotateCcw size={14} />
              </button>
            </>
          )}
          <div
            className="flex-1 text-xs font-mono text-[var(--text-secondary)] px-3 py-1.5 border border-[var(--border)] rounded-lg bg-[var(--surface-subtle)] truncate"
            title={browserUrl}
          >
            {browserUrl || t(locale, 'workshop.browserAddress')}
          </div>
          <button
            type="button"
            className="flex h-8 items-center gap-1.5 px-2 rounded-lg border border-[var(--border)] text-xs"
            onClick={async () => {
              try {
                await window.nativesAPI?.clipboard?.write?.(browserUrl);
                showToast(t(locale, 'workshop.copied'));
              } catch (err) {
                showToast(classifyError(err).userMessage);
              }
            }}
            title={t(locale, 'workshop.copyUrl')}
          >
            {t(locale, 'workshop.copyUrl')}
          </button>
          <button
            type="button"
            className="flex h-8 items-center gap-1.5 px-2 rounded-lg border border-[var(--border)] text-xs"
            onClick={async () => {
              if (!browserUrl) return;
              try {
                await window.nativesAPI?.shell?.openPath?.(browserUrl);
              } catch (err) {
                showToast(classifyError(err).userMessage);
              }
            }}
            title={t(locale, 'workshop.openSystemBrowser')}
          >
            <ExternalLink size={13} />
          </button>
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
                    resetLocalWizard();
                    setLocalWizardOpen(true);
                  }}
                >
                  <Folder size={14} className="text-[var(--text-secondary)]" />
                  <span>{t(locale, 'workshop.addMenuLocal')}</span>
                </button>
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
        <CreativeHome
          locale={locale}
          apps={apps}
          busyIds={busyIds}
          onReloadApps={() => { void reload(); }}
          onImport={() => setAddMenu('open')}
          onOpenApp={(app) => { void handleOpen(app); }}
          onStartApp={(app) => { void handleStart(app); }}
          onStopApp={(app) => { void handleStop(app); }}
          onDeleteApp={(app) => setDeleteTarget(app)}
          onRestartApp={(app) => { void handleRestart(app); }}
          onAppLogs={(app) => { void openLogs(app); }}
          onRunSettings={(app) => { void openEditLocal(app); }}
        />
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
              {deleteTarget.source === 'local_project'
                ? deleteLocalConfirmNote(locale === 'en' ? 'en' : 'zh')
                : t(locale, 'workshop.deleteDesc')}
            </p>
            <div className="p-3 bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg font-semibold text-sm text-[var(--text)]">
              {deleteTarget.title}
            </div>

            {deleteNeedsDockerOptions(deleteTarget.source) && (
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
        <Modal isOpen onClose={() => setLogsFor(null)} title={t(locale, 'workshop.logsTitle')} width={720}>
          <div className="py-1 flex flex-col gap-2">
            <div className="flex items-center justify-between gap-2 flex-wrap">
              <label className="flex items-center gap-2 text-[11px] text-[var(--text-secondary)]">
                <input
                  type="checkbox"
                  checked={logAutoScroll}
                  onChange={(e) => setLogAutoScroll(e.target.checked)}
                />
                {t(locale, 'workshop.logsAutoScroll')}
              </label>
              <input
                value={logFilter}
                onChange={(e) => setLogFilter(e.target.value)}
                placeholder={t(locale, 'workshop.logsFilter')}
                className="h-7 px-2 text-[11px] rounded border border-[var(--border)] bg-[var(--surface-subtle)] min-w-[160px]"
              />
              <div className="flex gap-2 flex-wrap">
                <button
                  type="button"
                  className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
                  onClick={async () => {
                    try {
                      await window.nativesAPI?.clipboard?.write?.(logsText || '');
                      showToast(t(locale, 'workshop.copied'));
                    } catch (err) {
                      showToast(classifyError(err).userMessage);
                    }
                  }}
                >
                  {t(locale, 'workshop.logsCopy')}
                </button>
                <button
                  type="button"
                  className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
                  onClick={() => setLogsText('')}
                >
                  {t(locale, 'workshop.logsClearView')}
                </button>
                <button
                  type="button"
                  className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
                  onClick={() => void openLogs(logsFor)}
                >
                  {t(locale, 'common.refresh')}
                </button>
                {logsFor.source === 'local_project' && (
                  <button
                    type="button"
                    className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
                    onClick={async () => {
                      try {
                        const d = await window.nativesAPI?.creativeApp?.diagnoseLocalWithAi?.(
                          logsFor.id,
                        );
                        if (d) {
                          showToast(`${d.issueCode}: ${d.summary}`);
                        }
                      } catch (err) {
                        showToast(classifyError(err).userMessage);
                      }
                    }}
                  >
                    {t(locale, 'workshop.localAiDiagnose')}
                  </button>
                )}
              </div>
            </div>
            <pre
              ref={logPreRef}
              className="max-h-96 overflow-auto text-[11px] font-mono bg-zinc-950 p-4 rounded-xl border border-zinc-800 leading-relaxed whitespace-pre-wrap"
            >
              {(logFilter
                ? logsText
                    .split('\n')
                    .filter((line) => line.toLowerCase().includes(logFilter.toLowerCase()))
                    .join('\n')
                : logsText
              )
                .split('\n')
                .map((line, i) => {
                  const color = line.includes('[stderr]')
                    ? 'text-rose-300'
                    : line.includes('[system]')
                      ? 'text-amber-200'
                      : 'text-zinc-200';
                  return (
                    <div key={i} className={color}>
                      {line}
                    </div>
                  );
                })}
            </pre>
          </div>
        </Modal>
      )}

      {localWizardOpen && (
        <Modal
          isOpen
          onClose={() => {
            setLocalWizardOpen(false);
            resetLocalWizard();
          }}
          title={t(locale, 'workshop.localWizardTitle')}
          width={560}
        >
          <div className="flex flex-col gap-4 py-1">
            <div className="flex gap-2 text-[11px] text-[var(--text-secondary)]">
              {(['basic', 'scan', 'launch', 'confirm'] as LocalWizardStep[]).map((s, i) => (
                <span
                  key={s}
                  className={
                    localStep === s
                      ? 'text-[var(--primary)] font-semibold'
                      : ''
                  }
                >
                  {i + 1}. {t(locale, `workshop.localStep.${s}` as 'workshop.localStep.basic')}
                </span>
              ))}
            </div>

            {localStep === 'basic' && (
              <div className="flex flex-col gap-3">
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                    {t(locale, 'workshop.localFolder')}
                  </label>
                  <div className="flex gap-2">
                    <input
                      value={localRoot}
                      onChange={(e) => setLocalRoot(e.target.value)}
                      className="flex-1 h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
                      placeholder="/path/to/project"
                    />
                    <button
                      type="button"
                      className="h-9 px-3 text-xs rounded-lg border border-[var(--border)]"
                      onClick={() => void pickLocalFolder()}
                    >
                      {t(locale, 'workshop.browseFiles')}
                    </button>
                  </div>
                </div>
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                    {t(locale, 'workshop.templateName')}
                  </label>
                  <input
                    value={localTitle}
                    onChange={(e) => setLocalTitle(e.target.value)}
                    className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                    {t(locale, 'workshop.description')}
                  </label>
                  <textarea
                    value={localDesc}
                    onChange={(e) => setLocalDesc(e.target.value)}
                    className="w-full min-h-[64px] px-3 py-2 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
                  />
                </div>
                {localScanError && (
                  <div className="text-xs text-rose-500">{localScanError}</div>
                )}
                <div className="flex justify-end gap-2">
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                    onClick={() => {
                      setLocalWizardOpen(false);
                      resetLocalWizard();
                    }}
                  >
                    {t(locale, 'common.cancel')}
                  </button>
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-white disabled:opacity-50"
                    disabled={!localRoot.trim() || localScanning}
                    onClick={() => void runLocalScan(localRoot.trim())}
                  >
                    {localScanning ? t(locale, 'workshop.localScanning') : t(locale, 'common.next')}
                  </button>
                </div>
              </div>
            )}

            {localStep === 'scan' && localScan && (
              <div className="flex flex-col gap-3 text-xs">
                <div className="grid grid-cols-2 gap-2">
                  <div>{t(locale, 'workshop.localKind')}: <strong>{localScan.projectKind}</strong></div>
                  <div>
                    {t(locale, 'workshop.localPm')}:{' '}
                    <strong>{localScan.packageManager || '—'}</strong>
                  </div>
                  <div>
                    node_modules:{' '}
                    <strong>{localScan.hasNodeModules ? 'yes' : 'missing'}</strong>
                  </div>
                  <div>
                    scripts:{' '}
                    <strong>{localScan.scripts.slice(0, 5).join(', ') || '—'}</strong>
                  </div>
                </div>
                {localScan.risks.length > 0 && (
                  <ul className="list-disc list-inside text-amber-600 dark:text-amber-400">
                    {localScan.risks.map((r) => (
                      <li key={r}>{r}</li>
                    ))}
                  </ul>
                )}
                {localScan.blockers.length > 0 && (
                  <ul className="list-disc list-inside text-rose-500">
                    {localScan.blockers.map((r) => (
                      <li key={r}>{r}</li>
                    ))}
                  </ul>
                )}
                <div className="flex justify-between gap-2">
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                    onClick={() => setLocalStep('basic')}
                  >
                    {t(locale, 'common.back')}
                  </button>
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-white disabled:opacity-50"
                    disabled={!canProceedFromScan(localScan)}
                    onClick={() => setLocalStep('launch')}
                  >
                    {t(locale, 'common.next')}
                  </button>
                </div>
              </div>
            )}

            {localStep === 'launch' && (
              <div className="flex flex-col gap-3 text-xs">
                <div className="grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    className={`h-9 rounded-lg border ${
                      localLaunchMode === 'smart'
                        ? 'border-[var(--primary)] text-[var(--primary)]'
                        : 'border-[var(--border)]'
                    }`}
                    onClick={() => setLocalLaunchMode('smart')}
                  >
                    {t(locale, 'workshop.localSmart')}
                  </button>
                  <button
                    type="button"
                    className={`h-9 rounded-lg border ${
                      localLaunchMode === 'custom'
                        ? 'border-[var(--primary)] text-[var(--primary)]'
                        : 'border-[var(--border)]'
                    }`}
                    onClick={() => setLocalLaunchMode('custom')}
                  >
                    {t(locale, 'workshop.localCustom')}
                  </button>
                </div>
                {localScan && localScan.packageManagerChoices.length > 1 && (
                  <div>
                    <label className="block mb-1">{t(locale, 'workshop.localPm')}</label>
                    <select
                      value={localPm || ''}
                      onChange={(e) => setLocalPm((e.target.value || undefined) as PackageManager | undefined)}
                      className="w-full h-9 px-2 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
                    >
                      <option value="">{t(locale, 'workshop.localChoosePm')}</option>
                      {localScan.packageManagerChoices.map((pm) => (
                        <option key={pm} value={pm}>
                          {pm}
                        </option>
                      ))}
                    </select>
                  </div>
                )}
                {localLaunchMode === 'custom' && localScan && localScan.scripts.length > 0 && (
                  <div>
                    <label className="block mb-1">script</label>
                    <select
                      value={localScript}
                      onChange={(e) => setLocalScript(e.target.value)}
                      className="w-full h-9 px-2 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
                    >
                      {localScan.scripts.map((s) => (
                        <option key={s} value={s}>
                          {s}
                        </option>
                      ))}
                    </select>
                  </div>
                )}
                {localLaunchMode === 'custom' && (
                  <div className="grid grid-cols-2 gap-2">
                    <div>
                      <label className="block mb-1">cwd</label>
                      <input
                        value={localCwd}
                        onChange={(e) => setLocalCwd(e.target.value)}
                        className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                      />
                    </div>
                    <div>
                      <label className="block mb-1">openPath</label>
                      <input
                        value={localOpenPath}
                        onChange={(e) => setLocalOpenPath(e.target.value)}
                        className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                      />
                    </div>
                    <div className="col-span-2">
                      <label className="block mb-1">port</label>
                      <div className="flex gap-2">
                        <select
                          value={localPortMode}
                          onChange={(e) => setLocalPortMode(e.target.value as 'auto' | 'fixed')}
                          className="h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                        >
                          <option value="auto">auto</option>
                          <option value="fixed">fixed</option>
                        </select>
                        {localPortMode === 'fixed' && (
                          <input
                            value={localPortValue}
                            onChange={(e) => setLocalPortValue(e.target.value)}
                            className="w-24 h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                            placeholder="5173"
                          />
                        )}
                      </div>
                    </div>
                  </div>
                )}
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={localAutoOpen}
                    onChange={(e) => setLocalAutoOpen(e.target.checked)}
                  />
                  {t(locale, 'workshop.localAutoOpen')}
                </label>
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="h-8 px-3 text-[11px] rounded-lg border border-[var(--border)] disabled:opacity-50"
                    disabled={localAiBusy || !localRoot.trim()}
                    onClick={async () => {
                      setLocalAiBusy(true);
                      setLocalAiPendingConfirm(false);
                      try {
                        const res = await window.nativesAPI?.creativeApp?.previewLocalAi?.(
                          localRoot.trim(),
                        );
                        if (res?.scan) setLocalScan(res.scan);
                        setLocalAiPreview(
                          res?.payloadPreview
                            ? JSON.stringify(res.payloadPreview, null, 2)
                            : null,
                        );
                        setLocalAiPendingConfirm(true);
                        showToast(t(locale, 'workshop.localAiPreviewReady'));
                      } catch (err) {
                        showToast(classifyError(err).userMessage);
                      } finally {
                        setLocalAiBusy(false);
                      }
                    }}
                  >
                    {localAiBusy ? t(locale, 'workshop.localScanning') : t(locale, 'workshop.localAiPreview')}
                  </button>
                  <button
                    type="button"
                    className="h-8 px-3 text-[11px] rounded-lg border border-[var(--border)] disabled:opacity-50"
                    disabled={localAiBusy || !localAiPendingConfirm || !localRoot.trim()}
                    onClick={async () => {
                      setLocalAiBusy(true);
                      try {
                        const res = await window.nativesAPI?.creativeApp?.analyzeLocalWithAi?.(
                          localRoot.trim(),
                          true,
                        );
                        if (res?.aiPlan) {
                          setLocalPlan(res.aiPlan);
                          setLocalLaunchMode('smart');
                        }
                        if (res?.scan) setLocalScan(res.scan);
                        showToast(
                          res?.aiPlan
                            ? t(locale, 'workshop.localAiPlanApplied')
                            : t(locale, 'workshop.localAiNoPlan'),
                        );
                        setLocalAiPendingConfirm(false);
                      } catch (err) {
                        showToast(classifyError(err).userMessage);
                      } finally {
                        setLocalAiBusy(false);
                      }
                    }}
                  >
                    {t(locale, 'workshop.localAiConfirmSend')}
                  </button>
                </div>
                {localAiPreview && (
                  <details className="text-[11px]" open={localAiPendingConfirm}>
                    <summary>{t(locale, 'workshop.localAiPayload')}</summary>
                    <pre className="mt-1 max-h-40 overflow-auto bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg p-2 whitespace-pre-wrap">
                      {localAiPreview}
                    </pre>
                  </details>
                )}
                <pre className="bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg p-3 whitespace-pre-wrap">
                  {planSummaryLines(
                    localLaunchMode === 'custom'
                      ? applyCustomPlanFromScan()
                      : localPlan ?? localScan?.rulePlan,
                    locale === 'en' ? 'en' : 'zh',
                  ).join('\n')}
                </pre>
                <div className="flex justify-between gap-2">
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                    onClick={() => setLocalStep('scan')}
                  >
                    {t(locale, 'common.back')}
                  </button>
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-white"
                    onClick={() => setLocalStep('confirm')}
                  >
                    {t(locale, 'common.next')}
                  </button>
                </div>
              </div>
            )}

            {localStep === 'confirm' && (
              <div className="flex flex-col gap-3 text-xs">
                <div className="p-3 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] space-y-1">
                  <div>
                    <strong>{localTitle || defaultLocalTitleFromPath(localRoot)}</strong>
                  </div>
                  <div className="text-[var(--text-secondary)] truncate">{localRoot}</div>
                  <pre className="whitespace-pre-wrap pt-2">
                    {planSummaryLines(
                      localLaunchMode === 'custom'
                        ? applyCustomPlanFromScan()
                        : localPlan ?? localScan?.rulePlan,
                      locale === 'en' ? 'en' : 'zh',
                    ).join('\n')}
                  </pre>
                </div>
                <label className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={localStartAfterSave}
                    onChange={(e) => setLocalStartAfterSave(e.target.checked)}
                  />
                  {t(locale, 'workshop.localStartAfterSave')}
                </label>
                <div className="flex justify-between gap-2">
                  <button
                    type="button"
                    className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                    onClick={() => setLocalStep('launch')}
                  >
                    {t(locale, 'common.back')}
                  </button>
                  <div className="flex gap-2">
                    <button
                      type="button"
                      className="h-9 px-4 text-xs rounded-lg border border-[var(--border)] disabled:opacity-50"
                      disabled={localSaving}
                      onClick={() => void saveLocalCreative(false)}
                    >
                      {t(locale, 'workshop.localSaveOnly')}
                    </button>
                    <button
                      type="button"
                      className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-white disabled:opacity-50"
                      disabled={localSaving}
                      onClick={() => void saveLocalCreative(true)}
                    >
                      {t(locale, 'workshop.localSaveStart')}
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        </Modal>
      )}

      {editLocal && (
        <Modal
          isOpen
          onClose={() => setEditLocal(null)}
          title={t(locale, 'workshop.actionEdit')}
          width={520}
        >
          <div className="flex flex-col gap-3 py-1 text-xs">
            {editLoading && <div>{t(locale, 'common.loading')}</div>}
            <div>
              <label className="block text-xs mb-1">{t(locale, 'workshop.templateName')}</label>
              <input
                value={editTitle}
                onChange={(e) => setEditTitle(e.target.value)}
                className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
              />
            </div>
            <label className="flex items-center gap-2 text-xs">
              <input
                type="checkbox"
                checked={editAutoOpen}
                onChange={(e) => setEditAutoOpen(e.target.checked)}
              />
              {t(locale, 'workshop.localAutoOpen')}
            </label>
            {editPlan && (
              <>
                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <label className="block mb-1">cwd</label>
                    <input
                      value={editCwd}
                      onChange={(e) => setEditCwd(e.target.value)}
                      className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                    />
                  </div>
                  <div>
                    <label className="block mb-1">script</label>
                    <input
                      value={editScript}
                      onChange={(e) => setEditScript(e.target.value)}
                      className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                    />
                  </div>
                  <div>
                    <label className="block mb-1">openPath</label>
                    <input
                      value={editOpenPath}
                      onChange={(e) => setEditOpenPath(e.target.value)}
                      className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                    />
                  </div>
                  <div>
                    <label className="block mb-1">port</label>
                    <div className="flex gap-1">
                      <select
                        value={editPortMode}
                        onChange={(e) => setEditPortMode(e.target.value as 'auto' | 'fixed')}
                        className="h-8 px-1 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                      >
                        <option value="auto">auto</option>
                        <option value="fixed">fixed</option>
                      </select>
                      {editPortMode === 'fixed' && (
                        <input
                          value={editPortValue}
                          onChange={(e) => setEditPortValue(e.target.value)}
                          className="w-20 h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                        />
                      )}
                    </div>
                  </div>
                </div>
                <div className="text-[11px] text-[var(--text-secondary)]">
                  env keys: {editEnvKeys.length ? editEnvKeys.join(', ') : '—'}
                </div>
                <pre className="p-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)] whitespace-pre-wrap">
                  {planSummaryLines(editPlan, locale === 'en' ? 'en' : 'zh').join('\n')}
                </pre>
              </>
            )}
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                onClick={() => setEditLocal(null)}
              >
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-white disabled:opacity-50"
                disabled={editSaving || editLoading}
                onClick={() => void saveEditLocal()}
              >
                {t(locale, 'common.save')}
              </button>
            </div>
          </div>
        </Modal>
      )}

      {depInstallFor && (
        <Modal
          isOpen
          onClose={() => setDepInstallFor(null)}
          title={t(locale, 'workshop.installDeps')}
          width={480}
        >
          <div className="flex flex-col gap-3 py-1 text-xs">
            <p className="text-[var(--text-secondary)]">{t(locale, 'workshop.installDepsWarn')}</p>
            {depCommand ? (
              <pre className="p-2 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] font-mono text-[11px] whitespace-pre-wrap">
                {depCommand}
              </pre>
            ) : null}
            <ul className="list-disc list-inside text-[var(--text-secondary)] space-y-1">
              <li>{t(locale, 'workshop.installDepsNetwork')}</li>
              <li>{t(locale, 'workshop.installDepsNodeModules')}</li>
              <li>{t(locale, 'workshop.installDepsLock')}</li>
              <li>{t(locale, 'workshop.installDepsUntrusted')}</li>
            </ul>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={depConfirmChecked}
                onChange={(e) => setDepConfirmChecked(e.target.checked)}
              />
              {t(locale, 'workshop.installDepsConfirm')}
            </label>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="h-9 px-4 rounded-lg border border-[var(--border)]"
                onClick={() => setDepInstallFor(null)}
              >
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="h-9 px-4 rounded-lg bg-[var(--primary)] text-white disabled:opacity-50"
                disabled={!depConfirmChecked || depInstalling}
                onClick={async () => {
                  if (!depInstallFor) return;
                  setDepInstalling(true);
                  try {
                    await window.nativesAPI?.creativeApp?.installLocalDependencies?.(
                      depInstallFor.id,
                    );
                    showToast(t(locale, 'workshop.installDepsDone'));
                    setDepInstallFor(null);
                    void reload();
                  } catch (err) {
                    showToast(classifyError(err).userMessage);
                  } finally {
                    setDepInstalling(false);
                  }
                }}
              >
                {t(locale, 'workshop.installDepsRun')}
              </button>
            </div>
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
