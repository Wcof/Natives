'use client';

import { useState, useEffect, useCallback } from 'react';
import { type AgentSession } from '@/types/agent';
import { t, type Locale } from '@/i18n';

interface ReplayStep {
  path: string;
  timestamp: number;
  action: string;
}

export default function SessionReplay() {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [selectedSession, setSelectedSession] = useState<AgentSession | null>(null);
  const [steps, setSteps] = useState<ReplayStep[]>([]);
  const [currentStep, setCurrentStep] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [loading, setLoading] = useState(true);
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

  // Load sessions
  const loadSessions = useCallback(async () => {
    setLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.agent?.scanProjects && api?.agent?.getSessions) {
        const projects = await api.agent.scanProjects();
        if (Array.isArray(projects) && projects.length > 0) {
          const result = await api.agent.getSessions(projects[0]!);
          if (Array.isArray(result)) {
            setSessions(result as AgentSession[]);
          }
        }
      }
    } catch (err) {
      console.error('[SessionReplay] Failed to load sessions:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  // When session selected, build replay steps
  useEffect(() => {
    if (!selectedSession) {
      setSteps([]);
      setCurrentStep(0);
      return;
    }
    // Build steps from session's modified files
    const fileSteps: ReplayStep[] = selectedSession.filesModified.map((f, i) => ({
      path: f,
      timestamp: selectedSession.startTime + i * 1000, // Approximate timestamps
      action: 'modify',
    }));
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
        padding: '8px 10px', borderBottom: '1px solid var(--border,#262920)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.sessionReplay')}
        </div>
      </div>

      {/* Session selector */}
      {!selectedSession && (
        <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
          {loading ? (
            <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
              {t(locale, 'common.loading')}
            </div>
          ) : sessions.length === 0 ? (
            <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
              {t(locale, 'aiWorkbench.selectSession')}
            </div>
          ) : (
            sessions.map((s) => (
              <div
                key={s.id}
                onClick={() => handleSelectSession(s)}
                style={{
                  padding: '8px 10px', marginBottom: 4, borderRadius: 6, cursor: 'pointer',
                  background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
                  transition: 'border-color 0.12s',
                }}
              >
                <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>{s.title}</div>
                <div style={{ fontSize: 10, color: 'var(--text-faint)' }}>
                  {s.engine} · {s.filesModified.length} {t(locale, 'aiWorkbench.files')} · {new Date(s.startTime).toLocaleDateString()}
                </div>
              </div>
            ))
          )}
        </div>
      )}

      {/* Replay view */}
      {selectedSession && steps.length > 0 && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: 8 }}>
          {/* Back button */}
          <button
            className="btn-ghost"
            onClick={() => { setSelectedSession(null); setPlaying(false); }}
            style={{ fontSize: 10, padding: '2px 6px', alignSelf: 'flex-start', marginBottom: 8 }}
          >
            ← {t(locale, 'common.cancel')}
          </button>

          {/* Timeline slider */}
          <div style={{ marginBottom: 8 }}>
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
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep(0)}
              style={{ fontSize: 11, padding: '2px 6px' }}
            >
              ⏮
            </button>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep((prev) => Math.max(0, prev - 1))}
              style={{ fontSize: 11, padding: '2px 6px' }}
            >
              ◀
            </button>
            <button
              className={`btn ${playing ? 'btn-primary' : 'btn-ghost'}`}
              onClick={() => setPlaying(!playing)}
              style={{ fontSize: 11, padding: '2px 10px' }}
            >
              {playing ? '⏸' : '▶'}
            </button>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep((prev) => Math.min(steps.length - 1, prev + 1))}
              style={{ fontSize: 11, padding: '2px 6px' }}
            >
              ▶
            </button>
            <button
              className="btn-ghost"
              onClick={() => setCurrentStep(steps.length - 1)}
              style={{ fontSize: 11, padding: '2px 6px' }}
            >
              ⏭
            </button>
            <span style={{ fontSize: 10, color: 'var(--text-faint)', marginLeft: 'auto' }}>
              {t(locale, 'aiWorkbench.step')} {currentStep + 1} {t(locale, 'aiWorkbench.of')} {steps.length}
            </span>
          </div>

          {/* Current step display */}
          {steps[currentStep] && (
            <div
              onClick={() => handleNavigateToFile(steps[currentStep]!.path)}
              style={{
                padding: 10, borderRadius: 6, cursor: 'pointer',
                background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
              }}
            >
              <div style={{ fontSize: 12, color: 'var(--accent,#cdf24b)', fontFamily: 'var(--font-mono)', marginBottom: 4 }}>
                {steps[currentStep]!.path.split('/').pop()}
              </div>
              <div style={{ fontSize: 10, color: 'var(--text-faint)' }}>
                {steps[currentStep]!.path}
              </div>
            </div>
          )}

          {/* File list */}
          <div style={{ marginTop: 8, fontSize: 10, color: 'var(--text-faint)' }}>
            {t(locale, 'aiWorkbench.files')} ({steps.length}):
          </div>
          <div style={{ flex: 1, overflow: 'auto', marginTop: 4 }}>
            {steps.map((step, i) => (
              <div
                key={step.path}
                onClick={() => { setCurrentStep(i); handleNavigateToFile(step.path); }}
                style={{
                  padding: '3px 8px', fontSize: 10, cursor: 'pointer', borderRadius: 3,
                  color: i === currentStep ? 'var(--accent,#cdf24b)' : 'var(--text-dim)',
                  background: i === currentStep ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
                }}
              >
                {step.path.split('/').pop()}
              </div>
            ))}
          </div>
        </div>
      )}

      {selectedSession && steps.length === 0 && (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
          No files modified in this session
        </div>
      )}
    </div>
  );
}
