'use client';

import { startTransition, useState, useEffect, useCallback } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { type AgentSession } from '@/types/agent';
import { t, type Locale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

interface ReplayStep {
  path: string;
  timestamp: number;
  action: string;
}

export default function SessionReplay() {
  const { data: sessions, loading, error, reload: loadSessions } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (api?.agent?.scanProjects && api?.agent?.getSessions) {
      const projects = await api.agent.scanProjects() as Array<{ path: string; name: string }>;
      if (Array.isArray(projects) && projects.length > 0) {
        const result = await api.agent.getSessions(projects[0]!.path);
        if (Array.isArray(result)) return result as AgentSession[];
      }
    }
    return [];
  }, []);
  const [selectedSession, setSelectedSession] = useState<AgentSession | null>(null);
  const [steps, setSteps] = useState<ReplayStep[]>([]);
  const [currentStep, setCurrentStep] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // When session selected, build replay steps
  useEffect(() => {
    if (!selectedSession) {
      startTransition(() => { setSteps([]); });
    // eslint-disable-next-line react-hooks/set-state-in-effect
      setCurrentStep(0);
      return;
    }
    // Build steps from session's modified files with real timestamps
    const paths = selectedSession.filesModified;
    const timestamps = selectedSession.fileTimestamps || {};
    const fileSteps: ReplayStep[] = paths.map((f, i) => ({
      path: f,
      timestamp: selectedSession.startTime + (timestamps[f] ?? i * 250),
      action: 'modify',
    }));
    // Sort by timestamp for chronological replay
    fileSteps.sort((a, b) => a.timestamp - b.timestamp);
    setSteps(fileSteps);
    setCurrentStep(0);
  }, [selectedSession]);

  // Auto-play
  useEffect(() => {
    if (!playing || steps.length === 0) return;
    const timer = setInterval(() => {
      setCurrentStep((prev) => {
        if (prev >= steps.length - 1) {
          setPlaying(false);
          return prev;
        }
        return prev + 1;
      });
    }, 800);
    return () => clearInterval(timer);
  }, [playing, steps.length]);

  const handleSelectSession = useCallback((session: AgentSession) => {
    setSelectedSession(session);
    setPlaying(false);
  }, []);

  const handleNavigateToFile = useCallback((path: string) => {
    const dir = path.substring(0, path.lastIndexOf('/')) || '/';
    window.dispatchEvent(new CustomEvent('navigate-files', { detail: dir }));
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--vibe-btn-border)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.sessionReplay')}
        </div>
      </div>

      {/* Session selector */}
      {!selectedSession && (
        <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
          {loading ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
              {t(locale, 'common.loading')}
            </div>
          ) : (sessions ?? []).length === 0 ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
              {t(locale, 'aiWorkbench.selectSession')}
            </div>
          ) : (
            (sessions ?? []).map((s) => (
              <div
                key={s.id}
                onClick={() => handleSelectSession(s)}
                style={{
                  padding: '8px 10px', marginBottom: SPACING.xs, borderRadius: BORDER_RADIUS.md, cursor: 'pointer',
                  background: 'var(--vibe-toolbar-bg)', border: '1px solid var(--vibe-btn-border)',
                  transition: 'border-color 0.12s',
                }}
              >
                <div style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text)' }}>{s.title}</div>
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)' }}>
                  {s.engine} · {s.filesModified.length} {t(locale, 'aiWorkbench.files')} · {new Date(s.startTime).toLocaleDateString()}
                </div>
              </div>
            ))
          )}
        </div>
      )}

      {/* Replay view */}
      {selectedSession && steps.length > 0 && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: 'var(--space-sm)' }}>
          {/* Back button */}
          <button
            className="btn-ghost"
            onClick={() => { setSelectedSession(null); setPlaying(false); }}
            style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', alignSelf: 'flex-start', marginBottom: SPACING.sm }}
          >
            ← {t(locale, 'common.cancel')}
          </button>

          {/* Timeline slider */}
          <div style={{ marginBottom: SPACING.sm }}>
            <input
              type="range"
              min={0}
              max={steps.length - 1}
              value={currentStep}
              onChange={(e) => { setCurrentStep(Number(e.target.value)); setPlaying(false); }}
              style={{ width: '100%' }}
            />
          </div>

          {/* Controls */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-sm)', marginBottom: SPACING.sm }}>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep(0)}
              style={{ fontSize: FONT_SIZE.sm, padding: '2px 6px' }}
            >
              ⏮
            </button>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep((prev) => Math.max(0, prev - 1))}
              style={{ fontSize: FONT_SIZE.sm, padding: '2px 6px' }}
            >
              ◀
            </button>
            <button
              className={`btn ${playing ? 'btn-primary' : 'btn-ghost'}`}
              onClick={() => setPlaying(!playing)}
              style={{ fontSize: FONT_SIZE.sm, padding: '2px 10px' }}
            >
              {playing ? '⏸' : '▶'}
            </button>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep((prev) => Math.min(steps.length - 1, prev + 1))}
              style={{ fontSize: FONT_SIZE.sm, padding: '2px 6px' }}
            >
              ▶
            </button>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep(steps.length - 1)}
              style={{ fontSize: FONT_SIZE.sm, padding: '2px 6px' }}
            >
              ⏭
            </button>
            <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)', marginLeft: 'auto' }}>
              {t(locale, 'aiWorkbench.step')} {currentStep + 1} {t(locale, 'aiWorkbench.of')} {steps.length}
            </span>
          </div>

          {/* Current step display */}
          {steps[currentStep] && (
            <div
              onClick={() => handleNavigateToFile(steps[currentStep]!.path)}
              style={{
                padding: 10, borderRadius: BORDER_RADIUS.md, cursor: 'pointer',
                background: 'var(--vibe-toolbar-bg)', border: '1px solid var(--vibe-btn-border)',
              }}
            >
              <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--accent)', fontFamily: 'var(--font-mono)', marginBottom: SPACING.xs }}>
                {steps[currentStep]!.path.split('/').pop()}
              </div>
              <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)' }}>
                {steps[currentStep]!.path}
              </div>
            </div>
          )}

          {/* File list */}
          <div style={{ marginTop: 8, fontSize: FONT_SIZE.xs, color: 'var(--text-faint)' }}>
            {t(locale, 'aiWorkbench.files')} ({steps.length}):
          </div>
          <div style={{ flex: 1, overflow: 'auto', marginTop: SPACING.xs }}>
            {steps.map((step, i) => (
              <div
                key={step.path}
                onClick={() => { setCurrentStep(i); handleNavigateToFile(step.path); }}
                style={{
                  padding: '3px 8px', fontSize: FONT_SIZE.xs, cursor: 'pointer', borderRadius: BORDER_RADIUS.sm,
                  color: i === currentStep ? 'var(--accent)' : 'var(--text-dim)',
                  background: i === currentStep ? 'var(--accent-soft)' : 'transparent',
                }}
              >
                {step.path.split('/').pop()}
              </div>
            ))}
          </div>
        </div>
      )}

      {selectedSession && steps.length === 0 && (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
          {t(locale, 'aiWorkbench.replay.noFiles')}
        </div>
      )}
    </div>
  );
}
