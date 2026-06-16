'use client';

import { useState, useEffect, useCallback } from 'react';
import { t, type Locale } from '@/i18n';

interface OnboardingWizardProps {
  locale: Locale;
  onComplete: () => void;
}

const STEPS = ['welcome', 'env-check', 'ai-config', 'module-install', 'done'] as const;
type Step = typeof STEPS[number];

export default function OnboardingWizard({ locale, onComplete }: OnboardingWizardProps) {
  const [step, setStep] = useState<Step>('welcome');
  const [envOk, setEnvOk] = useState<boolean | null>(null);
  const [apiKey, setApiKey] = useState('');
  const [checking, setChecking] = useState(false);

  const handleEnvCheck = useCallback(async () => {
    setChecking(true);
    try {
      const api = window.nativesAPI;
      // Check if we can read settings and env profiles
      const dbOk = await api?.db?.get?.('__health_check__').then(() => true).catch(() => false);
      const profiles = await api?.env?.listProfiles?.();
      setEnvOk(Boolean(dbOk && Array.isArray(profiles)));
    } catch {
      setEnvOk(false);
    } finally {
      setChecking(false);
    }
  }, []);

  const handleSaveApiKey = useCallback(async () => {
    if (!apiKey.trim()) return;
    try {
      const api = window.nativesAPI;
      // Store in a profile called "default"
      const profiles = await api?.env?.listProfiles?.();
      if (Array.isArray(profiles) && profiles.length === 0) {
        await api?.env?.createProfile?.('default');
      }
      await api?.env?.setVariable?.('default', 'ANTHROPIC_API_KEY', apiKey.trim());
    } catch { /* ignore */ }
  }, [apiKey]);

  const handleNext = useCallback(() => {
    const idx = STEPS.indexOf(step);
    const nextIdx = idx + 1;
    if (nextIdx < STEPS.length) {
      setStep(STEPS[nextIdx] as Step);
    }
  }, [step]);

  const handleSkip = useCallback(() => {
    onComplete();
  }, [onComplete]);

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 9999,
      background: 'var(--bg,#0b0c0a)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
    }}>
      <div style={{ width: 480, maxWidth: '90vw' }}>
        {/* Step indicator */}
        <div style={{ display: 'flex', gap: 6, marginBottom: 32, justifyContent: 'center' }}>
          {STEPS.map((s) => (
            <div key={s} style={{
              width: 8, height: 8, borderRadius: '50%',
              background: STEPS.indexOf(s) <= STEPS.indexOf(step) ? 'var(--accent,#cdf24b)' : 'var(--bg-3,#1c1e17)',
              transition: 'background 0.2s',
            }} />
          ))}
        </div>

        {/* Welcome */}
        {step === 'welcome' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🚀</div>
            <h1 style={{ fontSize: 22, fontWeight: 700, color: 'var(--text)', margin: '0 0 8px' }}>
              Welcome to Natives
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', lineHeight: 1.5, marginBottom: 24 }}>
              Your AI-native desktop app container. Let's get you set up in under 5 minutes.
            </p>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'center' }}>
              <button className="btn" onClick={handleSkip} style={{ fontSize: 12 }}>Skip</button>
              <button className="btn btn-primary" onClick={handleNext} style={{ fontSize: 12 }}>Get Started →</button>
            </div>
          </div>
        )}

        {/* Environment Check */}
        {step === 'env-check' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🔍</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              Environment Check
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
              Verifying your system is ready for Natives.
            </p>
            {envOk === null ? (
              <button className="btn btn-primary" onClick={handleEnvCheck} disabled={checking} style={{ fontSize: 12 }}>
                {checking ? 'Checking...' : 'Run Check'}
              </button>
            ) : envOk ? (
              <div>
                <div style={{ fontSize: 14, color: 'var(--accent,#cdf24b)', marginBottom: 16 }}>✅ All systems ready!</div>
                <button className="btn btn-primary" onClick={handleNext} style={{ fontSize: 12 }}>Continue →</button>
              </div>
            ) : (
              <div>
                <div style={{ fontSize: 14, color: 'var(--danger)', marginBottom: 16 }}>⚠️ Some checks failed — you can continue and fix later.</div>
                <button className="btn btn-primary" onClick={handleNext} style={{ fontSize: 12 }}>Continue Anyway →</button>
              </div>
            )}
          </div>
        )}

        {/* AI Config */}
        {step === 'ai-config' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🤖</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              AI Configuration
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 16 }}>
              Set your Anthropic API key to enable AI features. Your key is stored encrypted.
            </p>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-ant-..."
              style={{
                width: '100%', padding: '10px 12px', fontSize: 13,
                background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
                borderRadius: 6, color: 'var(--text)', outline: 'none', marginBottom: 16,
                fontFamily: 'var(--font-mono)',
              }}
            />
            <div style={{ display: 'flex', gap: 8, justifyContent: 'center' }}>
              <button className="btn" onClick={handleNext} style={{ fontSize: 12 }}>Skip</button>
              <button className="btn btn-primary" onClick={() => { handleSaveApiKey(); handleNext(); }} style={{ fontSize: 12 }} disabled={!apiKey.trim()}>
                Save & Continue
              </button>
            </div>
          </div>
        )}

        {/* Module Install */}
        {step === 'module-install' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🧩</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              Install Modules
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
              Drag & drop module ZIP files to install them, or skip and do it later from the Workshop.
            </p>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'center' }}>
              <button className="btn" onClick={handleNext} style={{ fontSize: 12 }}>Skip</button>
              <button className="btn btn-primary" onClick={() => { window.dispatchEvent(new CustomEvent('navigate', { detail: '__workshop__' })); onComplete(); }} style={{ fontSize: 12 }}>
                Open Workshop
              </button>
            </div>
          </div>
        )}

        {/* Done */}
        {step === 'done' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🎉</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              You're All Set!
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
              Natives is ready to use. Start by exploring the dashboard or opening the terminal.
            </p>
            <button className="btn btn-primary" onClick={onComplete} style={{ fontSize: 12 }}>
              Start Using Natives →
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
