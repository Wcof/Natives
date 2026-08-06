'use client';

import { useCallback, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type {
  CreativeAppInspectResult,
  CreativeAppInstallCandidate,
  CreativeAppProgressEvent,
} from '@/lib/tauri-adapter';

export type GithubWizardStep = 'url' | 'manual' | 'installing';

export interface GithubWizardCallbacks {
  onToast: (message: string) => void;
  /** Called after a successful install so the page can refresh the catalog. */
  onInstalled: () => void;
}

export interface GithubWizardState {
  step: GithubWizardStep;
  setStep: (s: GithubWizardStep) => void;
  repoUrl: string;
  setRepoUrl: (v: string) => void;
  tokenMode: 'saved' | 'once' | 'public';
  setTokenMode: (v: 'saved' | 'once' | 'public') => void;
  tokenInput: string;
  setTokenInput: (v: string) => void;
  saveToken: boolean;
  setSaveToken: (v: boolean) => void;
  inspecting: boolean;
  inspect: CreativeAppInspectResult | null;
  selectedTag: string;
  setSelectedTag: (v: string) => void;
  selectedCandidate: CreativeAppInstallCandidate | null;
  setSelectedCandidate: (c: CreativeAppInstallCandidate | null) => void;
  hostPort: string;
  setHostPort: (v: string) => void;
  openPath: string;
  setOpenPath: (v: string) => void;
  healthPath: string;
  setHealthPath: (v: string) => void;
  service: string;
  setService: (v: string) => void;
  envValues: Record<string, string>;
  setEnvValues: (v: Record<string, string>) => void;
  confirmBinds: boolean;
  setConfirmBinds: (v: boolean) => void;
  progress: CreativeAppProgressEvent | null;
  installError: string | null;
  reset: () => void;
  runInspect: (oneClick: boolean) => Promise<void>;
  runInstall: (
    inspectResult: CreativeAppInspectResult,
    cand: CreativeAppInstallCandidate,
    oneClick: boolean,
    override?: {
      releaseTag: string;
      hostPort: string;
      openPath: string;
      healthPath: string;
      service: string;
      envValues: Record<string, string>;
    },
  ) => Promise<void>;
  stageLabel: (stage: CreativeAppProgressEvent['stage']) => string;
  /** Subscribe to Host progress events (returns an unsubscribe). */
  onProgress: (ev: CreativeAppProgressEvent) => void;
}

/** GitHub install wizard state (T10). One state bundle for the URL/manual/
 * installing steps so a one-click install never reads stale tag/env state. */
export function useGithubWizardState({
  onToast,
  onInstalled,
}: GithubWizardCallbacks): GithubWizardState {
  const locale = useLocale();
  const [step, setStep] = useState<GithubWizardStep>('url');
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

  // Progress events stream in from the Host while installing.
  const onProgress = useCallback((ev: CreativeAppProgressEvent) => setProgress(ev), []);

  const reset = useCallback(() => {
    setStep('url');
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
  }, []);

  const tokenForRequest = useCallback((): string | undefined => {
    if (tokenMode === 'once' && tokenInput.trim()) return tokenInput.trim();
    return undefined;
  }, [tokenMode, tokenInput]);

  const runInstall = useCallback(
    async (
      inspectResult: CreativeAppInspectResult,
      cand: CreativeAppInstallCandidate,
      oneClick: boolean,
      override?: {
        releaseTag: string;
        hostPort: string;
        openPath: string;
        healthPath: string;
        service: string;
        envValues: Record<string, string>;
      },
    ) => {
      const form = override ?? {
        releaseTag: selectedTag,
        hostPort,
        openPath,
        healthPath,
        service,
        envValues,
      };
      setInstallError(null);
      setStep('installing');
      try {
        const api = window.nativesAPI?.creativeApp;
        await api?.installGithub?.({
          repositoryUrl: inspectResult.repositoryUrl,
          releaseTag: form.releaseTag || inspectResult.releaseTag,
          releaseId: inspectResult.releaseId,
          candidateId: cand.id,
          token: tokenForRequest(),
          hostPort: form.hostPort ? Number(form.hostPort) : cand.suggestedHostPort,
          openPath: form.openPath || cand.openPath,
          healthPath: form.healthPath || cand.healthPath,
          service: form.service || cand.service,
          env: Object.entries(form.envValues).map(([key, value]) => ({ key, value })),
          confirmBindMounts: confirmBinds || oneClick,
        });
        onToast(t(locale, 'workshop.githubInstallSuccess'));
        reset();
        onInstalled();
      } catch (err) {
        const message = classifyError(err).userMessage;
        setInstallError(message);
        setStep(oneClick ? 'url' : 'manual');
        // The wizard may have been closed (Esc/backdrop) mid-install — in that
        // case installError has nowhere to render, so toast it too.
        onToast(t(locale, 'workshop.installFailed') + ': ' + message);
      }
    },
    [
      confirmBinds,
      envValues,
      healthPath,
      hostPort,
      locale,
      onInstalled,
      onToast,
      openPath,
      reset,
      selectedTag,
      service,
      tokenForRequest,
    ],
  );

  const runInspect = useCallback(
    async (oneClick: boolean) => {
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
        if (
          oneClick &&
          res.candidates.length > 0 &&
          res.blockers.length === 0 &&
          res.candidates[0]
        ) {
          // Explicitly pass freshly-probed values: setState has not flushed in
          // the same tick, so reading state would emit the previous repo.
          await runInstall(res, res.candidates[0], true, {
            releaseTag: tag || res.releaseTag,
            hostPort: cand?.suggestedHostPort ? String(cand.suggestedHostPort) : '',
            openPath: cand?.openPath || '/',
            healthPath: cand?.healthPath || '',
            service: cand?.service || '',
            envValues: Object.fromEntries((cand?.envRequirements ?? []).map((e) => [e.key, ''])),
          });
        } else {
          setStep('manual');
        }
      } catch (err) {
        setInstallError(classifyError(err).userMessage);
      } finally {
        setInspecting(false);
      }
    },
    [repoUrl, runInstall, saveToken, selectedTag, tokenForRequest, tokenMode],
  );

  const stageLabel = useCallback(
    (stage: CreativeAppProgressEvent['stage']) => {
      const map: Record<string, string> = {
        inspecting_release: 'workshop.githubStageInspect',
        downloading_assets: 'workshop.githubStageDownload',
        pulling_image: 'workshop.githubStagePull',
        creating: 'workshop.githubStageCreate',
        starting: 'workshop.githubStageStart',
        installing_dependencies: 'workshop.githubStageDeps',
        health_check: 'workshop.githubStageHealth',
        ready: 'workshop.githubStageReady',
        failed: 'workshop.githubStageFailed',
        stopped: 'workshop.githubStageStopped',
      };
      const key = map[stage];
      return key ? t(locale, key) : stage;
    },
    [locale],
  );

  return {
    step,
    setStep,
    repoUrl,
    setRepoUrl,
    tokenMode,
    setTokenMode,
    tokenInput,
    setTokenInput,
    saveToken,
    setSaveToken,
    inspecting,
    inspect,
    selectedTag,
    setSelectedTag,
    selectedCandidate,
    setSelectedCandidate,
    hostPort,
    setHostPort,
    openPath,
    setOpenPath,
    healthPath,
    setHealthPath,
    service,
    setService,
    envValues,
    setEnvValues,
    confirmBinds,
    setConfirmBinds,
    progress,
    installError,
    reset,
    runInspect,
    runInstall,
    stageLabel,
    onProgress,
  };
}
