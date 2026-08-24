'use client';

import { useState, useEffect } from 'react';
import {
  aiApi,
  type OauthProviderPreset,
  type OauthSessionInfo,
} from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import {
  Zap,
  X,
  Check,
  Copy,
  Clock,
  ExternalLink,
  CheckCircle2,
} from 'lucide-react';

interface OAuthConnectModalProps {
  onClose: () => void;
  onSuccess: () => void;
}

export function OAuthConnectModal({ onClose, onSuccess }: OAuthConnectModalProps) {
  const locale = useLocale();
  const [presets, setPresets] = useState<OauthProviderPreset[]>([]);
  const [session, setSession] = useState<OauthSessionInfo | null>(null);
  const [manualCallbackUrl, setManualCallbackUrl] = useState('');
  const [polling, setPolling] = useState(false);
  const [copiedCode, setCopiedCode] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void aiApi.oauthListPresets().then(setPresets).catch((e) => setError(String(e)));
  }, []);

  const handleStart = async (preset: OauthProviderPreset) => {
    setError(null);
    try {
      const sess = await aiApi.oauthStart({ providerId: preset.providerId });
      setSession(sess);
      if (sess.flow === 'pkce' && sess.authorizeUrl) {
        window.open(sess.authorizeUrl, '_blank');
      } else if (sess.flow === 'device' && sess.verificationUri) {
        window.open(sess.verificationUri, '_blank');
      }
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    if (!session || session.flow !== 'device' || session.status === 'connected') {
      return;
    }
    const timer = setInterval(async () => {
      try {
        setPolling(true);
        const updated = await aiApi.oauthPoll(session.sessionId);
        setSession(updated);
        if (updated.status === 'connected') {
          clearInterval(timer);
          onSuccess();
        }
      } catch (err) {
        console.error('OAuth poll error:', err);
      } finally {
        setPolling(false);
      }
    }, (session.intervalSecs || 5) * 1000);

    return () => clearInterval(timer);
  }, [session, onSuccess]);

  const submitManualCallback = async () => {
    if (!manualCallbackUrl.trim()) return;
    setError(null);
    try {
      const updated = await aiApi.oauthSubmitCallback(manualCallbackUrl.trim());
      setSession(updated);
      setManualCallbackUrl('');
      onSuccess();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-xl p-6 shadow-2xl space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="font-bold text-lg flex items-center gap-2">
            <Zap className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'aiResources.oauthModal')}
          </h3>
          <button onClick={onClose} className="p-1 rounded text-[var(--muted-foreground)] hover:text-[var(--foreground)]">
            <X className="w-5 h-5" />
          </button>
        </div>

        {error && (
          <div className="p-3 rounded-lg bg-[var(--destructive)]/10 text-[var(--destructive)] text-xs">{error}</div>
        )}

        {!session ? (
          <div className="grid grid-cols-2 gap-3">
            {presets.map((preset) => (
              <button
                key={preset.providerId}
                onClick={() => handleStart(preset)}
                className="p-4 rounded-xl border border-[var(--border)] bg-[var(--card)] hover:border-[var(--primary)] hover:bg-[var(--primary)]/5 text-left transition flex flex-col justify-between"
              >
                <div>
                  <div className="font-bold text-sm">{preset.name}</div>
                  <div className="text-xs text-[var(--muted-foreground)] mt-1">Flow: {preset.flow.toUpperCase()}</div>
                </div>
                <div className="mt-4 text-xs text-[var(--primary)] font-semibold flex items-center gap-1">
                  {t(locale, 'aiResources.authorizeNow')} →
                </div>
              </button>
            ))}
          </div>
        ) : (
          <div className="space-y-4 p-4 rounded-xl bg-[var(--secondary)]/30 border border-[var(--border)]">
            <div className="flex items-center justify-between">
              <div className="font-bold text-sm">Session: {session.providerId.toUpperCase()}</div>
              <span className="text-xs font-semibold px-2.5 py-0.5 rounded-full bg-[var(--primary)]/10 text-[var(--primary)]">
                {session.status.toUpperCase()}
              </span>
            </div>

            {session.userCode && (
              <div className="p-4 rounded-xl bg-[var(--card)] border border-[var(--border)] text-center space-y-2">
                <div className="text-xs text-[var(--muted-foreground)]">{t(locale, 'aiResources.deviceUserCode')}</div>
                <div className="text-2xl font-mono font-bold tracking-widest text-[var(--primary)] flex items-center justify-center gap-2">
                  {session.userCode}
                  <button
                    onClick={() => {
                      navigator.clipboard.writeText(session.userCode || '');
                      setCopiedCode(true);
                      setTimeout(() => setCopiedCode(false), 2000);
                    }}
                    className="p-1 rounded text-xs text-[var(--muted-foreground)] hover:text-[var(--foreground)]"
                  >
                    {copiedCode ? <Check className="w-4 h-4 text-[var(--success)]" /> : <Copy className="w-4 h-4" />}
                  </button>
                </div>
                {session.verificationUri && (
                  <a
                    href={session.verificationUri}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1 text-xs text-[var(--primary)] hover:underline pt-1"
                  >
                    {t(locale, 'aiResources.openVerificationPage')}
                    <ExternalLink className="w-3 h-3" />
                  </a>
                )}
                <div className="text-[11px] text-[var(--muted-foreground)] pt-1 flex items-center justify-center gap-1">
                  <Clock className="w-3 h-3" />
                  {polling ? t(locale, 'aiResources.pollingStatus') : t(locale, 'aiResources.waitingConfirmation')}
                </div>
              </div>
            )}

            {session.flow === 'pkce' && (
              <div className="space-y-3">
                <div className="text-xs text-[var(--muted-foreground)]">
                  {t(locale, 'aiResources.callbackPasteHint')}
                </div>
                <div className="flex gap-2">
                  <input
                    type="text"
                    placeholder="http://localhost:54545/callback?code=..."
                    value={manualCallbackUrl}
                    onChange={(e) => setManualCallbackUrl(e.target.value)}
                    className="flex-1 px-3 py-1.5 bg-[var(--input)] border border-[var(--border)] rounded-lg text-xs font-mono"
                  />
                  <button
                    onClick={submitManualCallback}
                    className="px-3 py-1.5 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-xs font-medium"
                  >
                    {t(locale, 'aiResources.submit')}
                  </button>
                </div>
              </div>
            )}

            {session.status === 'connected' && (
              <div className="p-3 bg-[var(--success)]/10 border border-[var(--success)]/20 text-[var(--success)] rounded-lg text-xs flex items-center gap-2">
                <CheckCircle2 className="w-4 h-4" />
                {t(locale, 'aiResources.oauthSuccess')}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
