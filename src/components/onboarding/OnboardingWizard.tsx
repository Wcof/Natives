'use client';

import { useState, useEffect, useCallback } from 'react';
import { Search, Rocket } from 'lucide-react';
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
      background: 'var(--vibe-content-bg)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
    }}>
      <div style={{ width: 480, maxWidth: '90vw' }}>
        {/* Step indicator */}
        <div style={{ display: 'flex', gap: 6, marginBottom: 32, justifyContent: 'center' }}>
          {STEPS.map((s) => (
            <div key={s} style={{
              width: 8, height: 8, borderRadius: '50%',
              background: STEPS.indexOf(s) <= STEPS.indexOf(step) ? 'var(--accent)' : 'var(--vibe-btn-bg)',
              transition: 'background 0.2s',
            }} />
          ))}
        </div>

        {/* Welcome */}
        {step === 'welcome' && (
          <div style={{ textAlign: 'center' }}>
            <Rocket size={48} style={{ marginBottom: 16 }} />
            <h1 style={{ fontSize: 22, fontWeight: 700, color: 'var(--text)', margin: '0 0 8px' }}>
              {t(locale, 'onboarding.welcome')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', lineHeight: 1.5, marginBottom: 24 }}>
              {t(locale, 'onboarding.welcomeDesc')}
            </p>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'center' }}>
              <button className="btn" onClick={handleSkip} style={{ fontSize: 12 }}>{t(locale, 'onboarding.skip')}</button>
              <button className="btn btn-primary" onClick={handleNext} style={{ fontSize: 12 }}>{t(locale, 'onboarding.getStarted')}</button>
            </div>
          </div>
        )}

        {/* Environment Check */}
        {step === 'env-check' && (
          <div style={{ textAlign: 'center' }}>
            <Search size={48} style={{ color: 'var(--text-faint)', marginBottom: 16 }} />
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              {t(locale, 'onboarding.envCheck')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
              {t(locale, 'onboarding.envCheckDesc')}
            </p>
            {envOk === null ? (
              <button className="btn btn-primary" onClick={handleEnvCheck} disabled={checking} style={{ fontSize: 12 }}>
                {checking ? t(locale, 'onboarding.checking') : t(locale, 'onboarding.runCheck')}
              </button>
            ) : envOk ? (
              <div>
                <div style={{ fontSize: 14, color: 'var(--accent)', marginBottom: 16 }}>{t(locale, 'onboarding.allReady')}</div>
                <button className="btn btn-primary" onClick={handleNext} style={{ fontSize: 12 }}>{t(locale, 'onboarding.continue')}</button>
              </div>
            ) : (
              <div>
                <div style={{ fontSize: 14, color: 'var(--danger)', marginBottom: 16 }}>{t(locale, 'onboarding.checksFailed')}</div>
                <button className="btn btn-primary" onClick={handleNext} style={{ fontSize: 12 }}>{t(locale, 'onboarding.continueAnyway')}</button>
              </div>
            )}
          </div>
        )}

        {/* AI Config */}
        {step === 'ai-config' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🤖</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              {t(locale, 'onboarding.aiConfig')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 16 }}>
              {t(locale, 'onboarding.aiConfigDesc')}
            </p>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-ant-..."
              style={{
                width: '100%', padding: '10px 12px', fontSize: 13,
                background: 'var(--vibe-toolbar-bg)', border: '1px solid var(--vibe-btn-border)',
                borderRadius: 6, color: 'var(--text)', outline: 'none', marginBottom: 16,
                fontFamily: 'var(--font-mono)',
              }}
            />
            <div style={{ display: 'flex', gap: 8, justifyContent: 'center' }}>
              <button className="btn" onClick={handleNext} style={{ fontSize: 12 }}>{t(locale, 'onboarding.skip')}</button>
              <button className="btn btn-primary" onClick={() => { handleSaveApiKey(); handleNext(); }} style={{ fontSize: 12 }} disabled={!apiKey.trim()}>
                {t(locale, 'onboarding.saveAndContinue')}
              </button>
            </div>
          </div>
        )}

        {/* Module Install */}
        {step === 'module-install' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🧩</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              {t(locale, 'onboarding.moduleInstall')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
              {t(locale, 'onboarding.moduleInstallDesc')}
            </p>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'center' }}>
              <button className="btn" onClick={handleNext} style={{ fontSize: 12 }}>{t(locale, 'onboarding.skip')}</button>
              <button className="btn btn-primary" onClick={() => { window.dispatchEvent(new CustomEvent('navigate', { detail: '__workshop__' })); onComplete(); }} style={{ fontSize: 12 }}>
                {t(locale, 'onboarding.openWorkshop')}
              </button>
            </div>
          </div>
        )}

        {/* Done */}
        {step === 'done' && (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontSize: 48, marginBottom: 16 }}>🎉</div>
            <h1 style={{ fontSize: 20, fontWeight: 600, color: 'var(--text)', margin: '0 0 8px' }}>
              {t(locale, 'onboarding.allSet')}
            </h1>
            <p style={{ fontSize: 13, color: 'var(--text-dim)', marginBottom: 24 }}>
              {t(locale, 'onboarding.allSetDesc')}
            </p>
            <button className="btn btn-primary" onClick={onComplete} style={{ fontSize: 12 }}>
              {t(locale, 'onboarding.finish')}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
