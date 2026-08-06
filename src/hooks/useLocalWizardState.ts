'use client';

import { useCallback, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import {
  buildCreateRequest,
  defaultLocalTitleFromPath,
  pickPackageManager,
  type LocalWizardStep,
} from '@/lib/local-creative';
import type {
  LaunchPlan,
  LocalProjectScanResult,
  PackageManager,
} from '@/lib/tauri-adapter';

export interface LocalWizardCallbacks {
  onToast: (message: string) => void;
  /** Called after a successful create/save so the page can refresh the catalog. */
  onSaved: () => void;
}

export interface LocalWizardState {
  step: LocalWizardStep;
  setStep: (s: LocalWizardStep) => void;
  root: string;
  setRoot: (v: string) => void;
  title: string;
  setTitle: (v: string) => void;
  desc: string;
  setDesc: (v: string) => void;
  scanning: boolean;
  scan: LocalProjectScanResult | null;
  scanError: string | null;
  launchMode: 'smart' | 'custom';
  setLaunchMode: (v: 'smart' | 'custom') => void;
  plan: LaunchPlan | null;
  setPlan: (v: LaunchPlan | null) => void;
  pm: PackageManager | undefined;
  setPm: (v: PackageManager | undefined) => void;
  script: string;
  setScript: (v: string) => void;
  autoOpen: boolean;
  setAutoOpen: (v: boolean) => void;
  saving: boolean;
  cwd: string;
  setCwd: (v: string) => void;
  openPath: string;
  setOpenPath: (v: string) => void;
  portMode: 'auto' | 'fixed';
  setPortMode: (v: 'auto' | 'fixed') => void;
  portValue: string;
  setPortValue: (v: string) => void;
  aiPreview: string | null;
  aiBusy: boolean;
  aiPendingConfirm: boolean;
  reset: () => void;
  pickFolder: () => Promise<void>;
  runScan: (root: string) => Promise<void>;
  applyCustomPlan: () => LaunchPlan | null;
  save: (startAfter: boolean) => Promise<void>;
  runAiPreview: () => Promise<void>;
  runAiAnalyze: () => Promise<void>;
}

/**
 * Local project import wizard state (T10). All four steps share one state
 * bundle so a later step can never disagree with an earlier scan. The "start
 * after save" choice is made by which save button is pressed (saveOnly vs
 * saveStart) — there is no separate checkbox to drift from the buttons.
 */
export function useLocalWizardState({ onToast, onSaved }: LocalWizardCallbacks): LocalWizardState {
  const locale = useLocale();
  const [step, setStep] = useState<LocalWizardStep>('basic');
  const [root, setRoot] = useState('');
  const [title, setTitle] = useState('');
  const [desc, setDesc] = useState('');
  const [scanning, setScanning] = useState(false);
  const [scan, setScan] = useState<LocalProjectScanResult | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [launchMode, setLaunchMode] = useState<'smart' | 'custom'>('smart');
  const [plan, setPlan] = useState<LaunchPlan | null>(null);
  const [pm, setPm] = useState<PackageManager | undefined>(undefined);
  const [script, setScript] = useState('');
  const [autoOpen, setAutoOpen] = useState(true);
  const [saving, setSaving] = useState(false);
  const [cwd, setCwd] = useState('.');
  const [openPath, setOpenPath] = useState('/');
  const [portMode, setPortMode] = useState<'auto' | 'fixed'>('auto');
  const [portValue, setPortValue] = useState('');
  const [aiPreview, setAiPreview] = useState<string | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiPendingConfirm, setAiPendingConfirm] = useState(false);

  const reset = useCallback(() => {
    setStep('basic');
    setRoot('');
    setTitle('');
    setDesc('');
    setScanning(false);
    setScan(null);
    setScanError(null);
    setLaunchMode('smart');
    setPlan(null);
    setPm(undefined);
    setScript('');
    setAutoOpen(true);
    setSaving(false);
    setCwd('.');
    setOpenPath('/');
    setPortMode('auto');
    setPortValue('');
    setAiPreview(null);
    setAiBusy(false);
    setAiPendingConfirm(false);
  }, []);

  const pickFolder = useCallback(async () => {
    try {
      const path = await window.nativesAPI?.dialog?.pickDirectory?.();
      if (!path) return;
      setRoot(path);
      setTitle((prev) => prev.trim() || defaultLocalTitleFromPath(path));
    } catch (err) {
      onToast(classifyError(err).userMessage);
    }
  }, [onToast]);

  const runScan = useCallback(
    async (rootPath: string) => {
      setScanning(true);
      setScanError(null);
      try {
        const result = await window.nativesAPI?.creativeApp?.inspectLocal?.({ projectRoot: rootPath });
        if (!result) throw new Error('inspectLocal unavailable');
        setScan(result);
        setPm(pickPackageManager(result));
        setScript(result.preferredScript || result.scripts[0] || '');
        setPlan(result.rulePlan ?? null);
        if (result.existingId) onToast(t(locale, 'workshop.localAlreadyRegistered'));
        setStep('scan');
      } catch (err) {
        setScanError(classifyError(err).userMessage);
      } finally {
        setScanning(false);
      }
    },
    [locale, onToast],
  );

  const applyCustomPlan = useCallback((): LaunchPlan | null => {
    if (!scan) return null;
    const base = plan ?? scan.rulePlan;
    if (!base) {
      // Minimal static fallback when index.html exists via kind.
      if (scan.projectKind === 'html') {
        return {
          schemaVersion: 1,
          source: 'user',
          projectKind: 'html',
          runtime: 'static_http',
          program: 'internal',
          cwdRelative: cwd.trim() || '.',
          entryFile: 'index.html',
          args: [],
          environmentKeys: [],
          port: {
            mode: portMode,
            value: portMode === 'fixed' && portValue ? Number(portValue) : undefined,
          },
          openPath: openPath.trim() || '/',
          healthPath: openPath.trim() || '/',
          startupTimeoutMs: 60000,
          autoOpen,
          reason: 'user custom static',
        };
      }
      return null;
    }
    if (base.runtime === 'static_http') {
      return {
        ...base,
        source: 'user',
        cwdRelative: cwd.trim() || base.cwdRelative || '.',
        openPath: openPath.trim() || base.openPath || '/',
        healthPath: openPath.trim() || base.healthPath || '/',
        autoOpen,
        port: {
          mode: portMode,
          value: portMode === 'fixed' && portValue ? Number(portValue) : undefined,
        },
      };
    }
    const program = (pm || base.program) as LaunchPlan['program'];
    return {
      ...base,
      source: 'user',
      program: program === 'internal' ? 'npm' : program,
      script: script || base.script,
      cwdRelative: cwd.trim() || base.cwdRelative || '.',
      openPath: openPath.trim() || base.openPath || '/',
      healthPath: openPath.trim() || base.healthPath || '/',
      autoOpen,
      port: {
        mode: portMode,
        value: portMode === 'fixed' && portValue ? Number(portValue) : undefined,
      },
    };
  }, [autoOpen, cwd, openPath, plan, pm, portMode, portValue, scan, script]);

  const save = useCallback(
    async (startAfter: boolean) => {
      if (!root.trim()) return;
      setSaving(true);
      try {
        const mode = launchMode;
        const nextPlan =
          mode === 'custom' ? applyCustomPlan() : plan ?? scan?.rulePlan ?? null;
        if (!nextPlan) {
          onToast(t(locale, 'workshop.localNoPlan'));
          setSaving(false);
          return;
        }
        const req = buildCreateRequest({
          projectRoot: root.trim(),
          title,
          description: desc,
          launchMode: mode,
          launchPlan: nextPlan,
          autoOpen,
          packageManager: pm,
        });
        const created = await window.nativesAPI?.creativeApp?.createLocal?.(req);
        if (!created) throw new Error('createLocal failed');
        if (startAfter) {
          try {
            await window.nativesAPI?.creativeApp?.start?.(created.id);
          } catch (err) {
            onToast(classifyError(err).userMessage);
          }
        }
        onToast(t(locale, 'workshop.localSaved'));
        reset();
        onSaved();
      } catch (err) {
        onToast(classifyError(err).userMessage);
      } finally {
        setSaving(false);
      }
    },
    [applyCustomPlan, autoOpen, desc, launchMode, locale, onSaved, onToast, plan, pm, reset, root, scan, title],
  );

  const runAiPreview = useCallback(async () => {
    setAiBusy(true);
    setAiPendingConfirm(false);
    try {
      const res = await window.nativesAPI?.creativeApp?.previewLocalAi?.(root.trim());
      if (res?.scan) setScan(res.scan);
      setAiPreview(res?.payloadPreview ? JSON.stringify(res.payloadPreview, null, 2) : null);
      setAiPendingConfirm(true);
      onToast(t(locale, 'workshop.localAiPreviewReady'));
    } catch (err) {
      onToast(classifyError(err).userMessage);
    } finally {
      setAiBusy(false);
    }
  }, [locale, onToast, root]);

  const runAiAnalyze = useCallback(async () => {
    setAiBusy(true);
    try {
      const res = await window.nativesAPI?.creativeApp?.analyzeLocalWithAi?.(root.trim(), true);
      if (res?.aiPlan) {
        setPlan(res.aiPlan);
        setLaunchMode('smart');
      }
      if (res?.scan) setScan(res.scan);
      onToast(
        res?.aiPlan
          ? t(locale, 'workshop.localAiPlanApplied')
          : t(locale, 'workshop.localAiNoPlan'),
      );
      setAiPendingConfirm(false);
    } catch (err) {
      onToast(classifyError(err).userMessage);
    } finally {
      setAiBusy(false);
    }
  }, [locale, onToast, root]);

  return {
    step,
    setStep,
    root,
    setRoot,
    title,
    setTitle,
    desc,
    setDesc,
    scanning,
    scan,
    scanError,
    launchMode,
    setLaunchMode,
    plan,
    setPlan,
    pm,
    setPm,
    script,
    setScript,
    autoOpen,
    setAutoOpen,
    saving,
    cwd,
    setCwd,
    openPath,
    setOpenPath,
    portMode,
    setPortMode,
    portValue,
    setPortValue,
    aiPreview,
    aiBusy,
    aiPendingConfirm,
    reset,
    pickFolder,
    runScan,
    applyCustomPlan,
    save,
    runAiPreview,
    runAiAnalyze,
  };
}
