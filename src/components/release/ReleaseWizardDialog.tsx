'use client';

import { useState, useCallback } from 'react';
import { Check, AlertTriangle, X, ArrowRight, Play } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { useFocusTrap } from '@/lib/useFocusTrap';

interface ReleaseWizardDialogProps {
  locale: Locale;
  isOpen: boolean;
  onClose: () => void;
}

export default function ReleaseWizardDialog({ locale, isOpen, onClose }: ReleaseWizardDialogProps) {
  const trap = useFocusTrap();
  const [projectPath, setProjectPath] = useState('');
  const [inspection, setInspection] = useState<{
    hasPackageJson: boolean;
    hasGit: boolean;
    isClean: boolean;
    hasChangelog: boolean;
    hasGhCli: boolean;
    currentVersion: string;
    changelogHasUnreleased: boolean;
  } | null>(null);
  const [bumpType, setBumpType] = useState<'patch' | 'minor' | 'major'>('patch');
  const [newVersion, setNewVersion] = useState('');
  const [steps, setSteps] = useState<Array<{ name: string; command: string; description: string; enabled: boolean }>>([]);
  const [inspecting, setInspecting] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [log, setLog] = useState<string[]>([]);

  const handleInspect = useCallback(async () => {
    if (!projectPath.trim()) return;
    setInspecting(true);
    setInspection(null);
    try {
      const api = window.nativesAPI;
      if (!api?.release?.inspect) return;

      const result = await api.release.inspect(projectPath.trim());

      if ('error' in result && result.error) {
        setLog((prev) => [...prev, `Error: ${result.error}`]);
        return;
      }

      setInspection(result as typeof inspection);
      setLog((prev) => [...prev, `Inspected ${projectPath}`]);

      // Pre-fill version
      const insp = result as typeof inspection;
      const current = insp?.currentVersion || '0.0.0';
      const parts = current.split('.').map(Number);
      let next: string;
      if (bumpType === 'patch') next = `${parts[0] ?? 0}.${parts[1] ?? 0}.${(parts[2] ?? 0) + 1}`;
      else if (bumpType === 'minor') next = `${parts[0] ?? 0}.${(parts[1] ?? 0) + 1}.0`;
      else next = `${(parts[0] ?? 0) + 1}.0.0`;
      setNewVersion(next);
    } catch (err) {
      setLog((prev) => [...prev, `Inspection failed: ${(err as Error).message}`]);
    } finally {
      setInspecting(false);
    }
  }, [projectPath, bumpType]);

  const handlePrepare = useCallback(async () => {
    if (!projectPath.trim() || !newVersion) return;
    setPreparing(true);
    try {
      const api = window.nativesAPI;
      if (!api?.release?.prepare) return;

      const result = await api.release.prepare(projectPath.trim(), newVersion);
      if (result.success) {
        setLog((prev) => [...prev, `Prepared release v${newVersion}`]);

        // Get command sequence
        const seq = await api.release.getSequence(projectPath.trim(), newVersion);
        if (seq.steps) setSteps(seq.steps);
      } else {
        setLog((prev) => [...prev, `Prepare failed: ${result.error || 'Unknown error'}`]);
      }
    } catch (err) {
      setLog((prev) => [...prev, `Prepare failed: ${(err as Error).message}`]);
    } finally {
      setPreparing(false);
    }
  }, [projectPath, newVersion]);

  const handleExecuteStep = useCallback(async (step: { name: string; command: string }) => {
    setLog((prev) => [...prev, `> ${step.command}`]);
    try {
      const api = window.nativesAPI;
      if (!api?.release?.execute) return;

      const result = await api.release.execute(projectPath.trim(), step.command);
      if (result.success) {
        setLog((prev) => [...prev, `✓ ${step.name} completed`]);
      } else {
        setLog((prev) => [...prev, `✗ ${step.name} failed: ${result.error || 'Unknown'}`]);
      }
    } catch (err) {
      setLog((prev) => [...prev, `✗ ${step.name} error: ${(err as Error).message}`]);
    }
  }, [projectPath]);

  const handleRunAll = useCallback(async () => {
    for (const step of steps) {
      if (step.enabled) {
        await handleExecuteStep(step);
      }
    }
  }, [steps, handleExecuteStep]);

  if (!isOpen) return null;

  return (
    <div
      ref={trap.dialogRef}
      role="dialog"
      aria-modal="true"
      aria-label={t(locale, 'release.title')}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 9000,
        background: 'rgba(0,0,0,0.5)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
      onKeyDown={(e) => {
        trap.handleKeyDown(e);
        if (e.key === 'Escape') onClose();
      }}
    >
      <div
        style={{
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 12,
          width: 600,
          maxHeight: '80vh',
          overflow: 'auto',
          padding: 24,
        }}
      >
        {/* Header */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 20 }}>
          <h2 style={{ fontSize: 18, fontWeight: 600, color: 'var(--text)', margin: 0 }}>
            {t(locale, 'release.title')}
          </h2>
          <button onClick={onClose} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-faint)' }}>
            <X size={18} />
          </button>
        </div>

        {/* Project path */}
        <div style={{ marginBottom: 16 }}>
          <label style={{ fontSize: 12, color: 'var(--text-dim)', marginBottom: 4, display: 'block' }}>
            {t(locale, 'release.projectPath')}
          </label>
          <div style={{ display: 'flex', gap: 8 }}>
            <input
              value={projectPath}
              onChange={(e) => setProjectPath(e.target.value)}
              placeholder="/path/to/your/project"
              style={{
                flex: 1,
                background: 'var(--bg)',
                border: '1px solid var(--border)',
                borderRadius: 6,
                padding: '8px 12px',
                color: 'var(--text)',
                fontSize: 13,
              }}
            />
            <button className="btn btn-primary" onClick={handleInspect} disabled={inspecting || !projectPath.trim()}>
              {inspecting ? t(locale, 'common.loading') : t(locale, 'release.inspectProject')}
            </button>
          </div>
        </div>

        {/* Inspection results */}
        {inspection && (
          <div style={{ marginBottom: 20 }}>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
              <StatusItem label={t(locale, 'release.packageJson')} ok={inspection.hasPackageJson} />
              <StatusItem label={t(locale, 'release.gitStatus')} ok={inspection.isClean}
                detail={inspection.isClean ? t(locale, 'release.clean') : t(locale, 'release.uncommitted')} />
              <StatusItem label={t(locale, 'release.changelog')} ok={inspection.hasChangelog}
                detail={inspection.changelogHasUnreleased ? t(locale, 'release.hasUnreleased') : t(locale, 'release.noUnreleased')} />
              <StatusItem label={t(locale, 'release.ghCli')} ok={inspection.hasGhCli}
                detail={inspection.hasGhCli ? t(locale, 'release.installed') : t(locale, 'release.notInstalled')} />
            </div>

            <div style={{ marginTop: 12, fontSize: 13, color: 'var(--text)' }}>
              {t(locale, 'release.version')}: <strong>{inspection.currentVersion}</strong>
            </div>

            {/* Version bump */}
            <div style={{ marginTop: 12, display: 'flex', alignItems: 'center', gap: 8 }}>
              <select
                value={bumpType}
                onChange={(e) => setBumpType(e.target.value as 'patch' | 'minor' | 'major')}
                style={{
                  background: 'var(--bg)',
                  border: '1px solid var(--border)',
                  borderRadius: 6,
                  padding: '6px 10px',
                  color: 'var(--text)',
                  fontSize: 12,
                }}
              >
                <option value="patch">{t(locale, 'release.patch')}</option>
                <option value="minor">{t(locale, 'release.minor')}</option>
                <option value="major">{t(locale, 'release.major')}</option>
              </select>
              <ArrowRight size={14} style={{ color: 'var(--text-faint)' }} />
              <input
                value={newVersion}
                onChange={(e) => setNewVersion(e.target.value)}
                style={{
                  background: 'var(--bg)',
                  border: '1px solid var(--border)',
                  borderRadius: 6,
                  padding: '6px 10px',
                  color: 'var(--text)',
                  fontSize: 12,
                  width: 100,
                }}
              />
              <button className="btn btn-primary btn-sm" onClick={handlePrepare} disabled={preparing || !newVersion}>
                {preparing ? t(locale, 'common.loading') : t(locale, 'release.prepareRelease')}
              </button>
            </div>
          </div>
        )}

        {/* Steps */}
        {steps.length > 0 && (
          <div style={{ marginBottom: 16 }}>
            <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 8 }}>
              {t(locale, 'release.commandSequence')}
            </div>
            {steps.map((step, i) => (
              <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '6px 0', borderBottom: '1px solid var(--border)' }}>
                <input
                  type="checkbox"
                  checked={step.enabled}
                  onChange={() => {
                    setSteps((prev) => prev.map((s, j) => j === i ? { ...s, enabled: !s.enabled } : s));
                  }}
                />
                <span style={{ fontSize: 12, color: 'var(--text)', flex: 1 }}>{step.description}</span>
                <button className="btn btn-sm" onClick={() => handleExecuteStep(step)} disabled={!step.enabled}>
                  <Play size={12} /> {t(locale, 'release.runSteps')}
                </button>
              </div>
            ))}
            <button className="btn btn-primary" onClick={handleRunAll} style={{ marginTop: 8, width: '100%' }}>
              {t(locale, 'release.execute')}
            </button>
          </div>
        )}

        {/* Log */}
        {log.length > 0 && (
          <div
            style={{
              background: 'var(--bg)',
              border: '1px solid var(--border)',
              borderRadius: 6,
              padding: 8,
              maxHeight: 120,
              overflow: 'auto',
              fontSize: 11,
              fontFamily: 'monospace',
              color: 'var(--text-dim)',
            }}
          >
            {log.map((line, i) => (
              <div key={i}>{line}</div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function StatusItem({ label, ok, detail }: { label: string; ok: boolean; detail?: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 8px', borderRadius: 6, background: ok ? 'rgba(68,204,68,0.08)' : 'rgba(255,68,68,0.08)' }}>
      {ok ? (
        <Check size={14} style={{ color: '#4c4' }} />
      ) : (
        <AlertTriangle size={14} style={{ color: '#f44' }} />
      )}
      <span style={{ fontSize: 11, color: 'var(--text)' }}>{label}</span>
      {detail && <span style={{ fontSize: 10, color: 'var(--text-faint)' }}>({detail})</span>}
    </div>
  );
}
