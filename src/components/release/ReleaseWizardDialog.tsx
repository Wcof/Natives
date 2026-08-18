'use client';

import { useEffect, useRef, useState } from 'react';
import { AlertCircle, Check, FolderOpen, GitBranch, Search, X } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { t as tr, type Locale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import { readActiveProject } from '@/lib/active-project';
import type { ReleaseAction, ReleaseExecution, ReleasePlan, ReleaseStep } from '@/lib/tauri/types-api';

/**
 * 发布向导 — 驱动真实后端链路：
 * release.inspect（项目体检）→ release.prepare（改版本号）→
 * release.getSequence（命令序列）→ release.execute（逐条执行，失败即停）。
 * 此前版本为纯前端模拟（setTimeout 假进度 + 假“已发布”），已重写。
 */

interface ProjectInspection {
  name: string;
  version: string;
  hasChangelog: boolean;
  hasPackageJson: boolean;
  hasCargoToml: boolean;
  gitDirty: boolean;
  gitBranch: string;
}

type StepStatus = 'pending' | 'running' | 'ok' | 'fail';

interface ReleaseWizardDialogProps {
  locale: Locale;
  isOpen: boolean;
  onClose: () => void;
}

type WizardStep = 'inspect' | 'plan' | 'run' | 'done';

function bumpPatch(version: string): string {
  const m = version.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!m) return version;
  return `${m[1]}.${m[2]}.${Number(m[3]) + 1}`;
}

export default function ReleaseWizardDialog({ locale, isOpen, onClose }: ReleaseWizardDialogProps) {
  const t = (key: string) => tr(locale, key);

  const [step, setStep] = useState<WizardStep>('inspect');
  const [projectPath, setProjectPath] = useState('');
  const [inspecting, setInspecting] = useState(false);
  const [inspection, setInspection] = useState<ProjectInspection | null>(null);
  const [newVersion, setNewVersion] = useState('');
  const [sequence, setSequence] = useState<ReleaseStep[]>([]);
  const [stepStatus, setStepStatus] = useState<Partial<Record<ReleaseAction, StepStatus>>>({});
  const [stepError, setStepError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancelledRef = useRef(false);

  // 打开时重置并预填活动项目路径
  useEffect(() => {
    if (!isOpen) return;
    cancelledRef.current = false;
    setStep('inspect');
    setInspection(null);
    setSequence([]);
    setStepStatus({});
    setStepError(null);
    setNewVersion('');
    setError(null);
    let stale = false;
    readActiveProject(window.nativesAPI).then((p) => {
      if (!stale && p) setProjectPath((prev) => prev || p);
    }).catch(() => { /* 无活动项目时留空让用户选 */ });
    return () => { stale = true; cancelledRef.current = true; };
  }, [isOpen]);

  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;

  const handlePickDirectory = async () => {
    const picked = await api?.dialog?.pickDirectory?.();
    if (picked) setProjectPath(picked);
  };

  const handleInspect = async () => {
    if (!projectPath.trim() || !api?.release?.inspect) return;
    setInspecting(true);
    setError(null);
    try {
      const result = (await api.release.inspect(projectPath.trim())) as ProjectInspection;
      setInspection(result);
      setNewVersion(bumpPatch(result.version));
    } catch (e) {
      setInspection(null);
      setError(classifyError(e).userMessage);
    } finally {
      setInspecting(false);
    }
  };

  const handlePlan = async () => {
    if (!newVersion.trim() || !api?.release?.getSequence) return;
    setError(null);
    try {
      const result: ReleasePlan = await api.release.getSequence(projectPath.trim(), newVersion.trim());
      setSequence(result.steps ?? []);
      setStepStatus({});
      setStepError(null);
      setStep('plan');
    } catch (e) {
      setError(classifyError(e).userMessage);
    }
  };

  const handleRun = async () => {
    if (!api?.release?.prepare || !api?.release?.execute) return;
    setRunning(true);
    setStep('run');
    setStepError(null);
    try {
      for (const s of sequence) {
        if (cancelledRef.current) return;
        if (stepStatus[s.action] === 'ok') continue;
        setStepStatus((prev) => ({ ...prev, [s.action]: 'running' }));
        if (s.action === 'update-version') {
          // 版本号写入走 prepare（package.json / Cargo.toml）
          await api.release.prepare(projectPath.trim(), newVersion.trim());
          setStepStatus((prev) => ({ ...prev, [s.action]: 'ok' }));
          continue;
        }
        const result: ReleaseExecution = await api.release.execute(projectPath.trim(), newVersion.trim(), s.action);
        if (!result.success) {
          setStepStatus((prev) => ({ ...prev, [s.action]: 'fail' }));
          setStepError((result.stderr || result.stdout || '').trim().slice(-800) || `exit ${result.exitCode}`);
          setRunning(false);
          return;
        }
        setStepStatus((prev) => ({ ...prev, [s.action]: 'ok' }));
      }
      setStep('done');
    } catch (e) {
      setStepError(classifyError(e).userMessage);
    } finally {
      setRunning(false);
    }
  };

  const handleClose = () => {
    if (running) return;
    onClose();
  };

  const inputStyle = { width: '100%' } as const;

  return (
    <Modal
      isOpen={isOpen}
      onClose={handleClose}
      title={t('release.title')}
      width={520}
      showCloseButton={!running}
      closeOnBackdropClick={!running}
      closeOnEscape={!running}
    >
      {/* Step: inspect — 选项目并体检 */}
      {step === 'inspect' && (
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-xs font-medium text-[var(--text-secondary)]">
              {t('release.projectPath')}
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={projectPath}
                onChange={(e) => setProjectPath(e.target.value)}
                placeholder="/path/to/project"
                className="input w-full"
                aria-label={t('release.projectPath')}
              />
              <button type="button" className="btn" onClick={handlePickDirectory} title={t('release.selectProject')}>
                <FolderOpen size={14} />
              </button>
            </div>
          </div>

          <button
            type="button"
            className="btn btn-primary btn-sm"
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
            disabled={!projectPath.trim() || inspecting}
            onClick={handleInspect}
          >
            <Search size={13} /> {t('release.inspectProject')}
          </button>

          {inspecting && <MathCurveLoader size={24} />}

          {inspection && (
            <div className="space-y-1.5 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3 text-sm">
              <InspectRow label={t('release.projectPath')} value={inspection.name} />
              <InspectRow label={t('release.version')} value={`v${inspection.version}`} mono />
              <InspectRow
                label={t('release.gitStatus')}
                value={`${inspection.gitBranch} · ${inspection.gitDirty ? t('release.uncommitted') : t('release.clean')}`}
                warn={inspection.gitDirty}
                icon={<GitBranch size={12} />}
              />
              <InspectRow
                label={t('release.changelog')}
                value={inspection.hasChangelog ? t('release.present') : t('release.missing')}
                warn={!inspection.hasChangelog}
              />
              <InspectRow
                label={t('release.packageJson')}
                value={[
                  inspection.hasPackageJson ? 'package.json' : null,
                  inspection.hasCargoToml ? 'Cargo.toml' : null,
                ].filter(Boolean).join(' + ') || t('release.missing')}
                warn={!inspection.hasPackageJson && !inspection.hasCargoToml}
              />
            </div>
          )}

          {inspection && (
            <div>
              <label className="mb-1.5 block text-xs font-medium text-[var(--text-secondary)]">
                {t('release.newVersion')}
              </label>
              <input
                type="text"
                value={newVersion}
                onChange={(e) => setNewVersion(e.target.value)}
                placeholder={bumpPatch(inspection.version)}
                className="input"
                style={inputStyle}
                aria-label={t('release.newVersion')}
              />
            </div>
          )}

          {error && <ErrorRow message={error} />}
        </div>
      )}

      {/* Step: plan — 展示命令序列 */}
      {(step === 'plan' || step === 'run' || step === 'done') && (
        <div className="space-y-3">
          <p className="text-xs text-[var(--text-secondary)]">
            {t('release.commandSequence')} · v{newVersion}
          </p>
          <div className="space-y-1 rounded-lg border border-[var(--border)] bg-[var(--surface)] p-2">
            {sequence.map((s) => {
              const status = stepStatus[s.action] ?? 'pending';
              return (
                <div key={s.action} className="flex items-center gap-2 px-1.5 py-1 text-sm text-[var(--text)]">
                  <span style={{ display: 'inline-flex', width: 14, flexShrink: 0 }}>
                    {status === 'ok' && <Check size={13} style={{ color: 'var(--diff-add)' }} />}
                    {status === 'fail' && <X size={13} style={{ color: 'var(--danger)' }} />}
                    {status === 'running' && <MathCurveLoader size={13} strokeWidth={1} particleCount={6} />}
                  </span>
                  <span className="flex-1">{s.label}</span>
                  <code className="text-xs text-[var(--text-disabled)]" style={{ fontFamily: 'var(--font-mono)' }}>{s.display}</code>
                </div>
              );
            })}
          </div>

          {stepError && <ErrorRow message={`${t('release.stepFailed')}: ${stepError}`} />}
          {step === 'done' && (
            <p className="flex items-center gap-2 text-sm" style={{ color: 'var(--diff-add)' }}>
              <Check size={14} /> {t('release.done')}
            </p>
          )}
        </div>
      )}

      {/* Footer */}
      <div className="mt-5 flex items-center justify-end gap-2 border-t border-[var(--border)] pt-3">
        <button type="button" className="btn btn-ghost" onClick={handleClose} disabled={running}>
          {tr(locale, step === 'done' ? 'common.close' : 'common.cancel')}
        </button>
        {step === 'inspect' && (
          <button
            type="button"
            className="btn btn-primary"
            disabled={!inspection || !newVersion.trim()}
            onClick={handlePlan}
          >
            {t('release.prepareRelease')}
          </button>
        )}
        {step === 'plan' && (
          <button type="button" className="btn btn-primary" disabled={running} onClick={handleRun}>
            {t('release.runSteps')}
          </button>
        )}
        {step === 'run' && stepError && (
          <button type="button" className="btn btn-primary" disabled={running} onClick={handleRun}>
            {t('release.runSteps')}
          </button>
        )}
      </div>
    </Modal>
  );
}

function InspectRow({ label, value, warn, mono, icon }: {
  label: string; value: string; warn?: boolean; mono?: boolean; icon?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-28 flex-shrink-0 text-xs text-[var(--text-secondary)]">{label}</span>
      <span
        className="flex items-center gap-1 text-xs"
        style={{
          color: warn ? 'var(--warning)' : 'var(--text)',
          fontFamily: mono ? 'var(--font-mono)' : undefined,
        }}
      >
        {icon}{value}
      </span>
    </div>
  );
}

function ErrorRow({ message }: { message: string }) {
  return (
    <div
      className="flex items-start gap-2 rounded-lg px-3 py-2 text-xs"
      style={{ background: 'var(--danger-soft)', color: 'var(--danger)' }}
    >
      <AlertCircle size={14} style={{ flexShrink: 0, marginTop: 1 }} />
      <span style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{message}</span>
    </div>
  );
}
