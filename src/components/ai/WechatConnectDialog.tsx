'use client';

import { useEffect, useState, useCallback, useRef } from 'react';
import { QrCode, Send, Wifi, WifiOff, RefreshCw } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';

interface WechatConnectDialogProps {
  onClose: () => void;
}

type ConnState =
  | 'uninstalled'
  | 'installed_off'
  | 'gateway_online'
  | 'selecting_agent'
  | 'qr_generated'
  | 'qr_showing'
  | 'connected'
  | 'expired'
  | 'canceled'
  | 'wait'
  | 'unreachable';

interface BridgeEnv {
  target: string;
  cwd: string;
  persona: string;
  state: ConnState | string;
  connected: boolean;
}

interface QrLoginState {
  token: string;
  image: string;
}

function stateLabel(locale: string, state: string | undefined): string {
  if (!state) return t(locale, 'wechat.disconnected');
  const key = `wechat.state.${state}`;
  const translated = t(locale, key);
  return translated === key ? state : translated;
}

function buildUserError(locale: string, titleKey: string, error: unknown): string {
  const classified = classifyError(error);
  return `${t(locale, titleKey)}: ${classified.userMessage}. ${classified.actionHint}`;
}

export default function WechatConnectDialog({ onClose }: WechatConnectDialogProps) {
  const locale = useLocale();
  const { toast } = useToast();
  const api = typeof window !== 'undefined' ? window.nativesAPI?.wechat : undefined;
  const mountedRef = useRef(true);
  const [env, setEnv] = useState<BridgeEnv | null>(null);
  const [qrcode, setQrcode] = useState<QrLoginState | null>(null);
  const [envLoading, setEnvLoading] = useState(true);
  const [loginLoading, setLoginLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [message, setMessage] = useState('');
  const [messages, setMessages] = useState<Array<{ role: string; text: string }>>([]);

  const showError = useCallback((titleKey: string, error: unknown) => {
    if (!mountedRef.current) return;
    const message = buildUserError(locale, titleKey, error);
    setErrorMessage(message);
    toast(message, 'error');
  }, [locale, toast]);

  const refreshEnv = useCallback(async (showFailure = true) => {
    if (!api) {
      if (!mountedRef.current) return;
      setErrorMessage(t(locale, 'wechat.unavailable'));
      setEnvLoading(false);
      return;
    }
    try {
      const e = await api.env();
      if (!mountedRef.current) return;
      setEnv(e);
      setErrorMessage(null);
    } catch (error) {
      if (showFailure) showError('wechat.loadFailed', error);
    } finally {
      if (mountedRef.current) setEnvLoading(false);
    }
  }, [api, locale, showError]);

  useEffect(() => {
    mountedRef.current = true;
    refreshEnv(true);
    return () => { mountedRef.current = false; };
  }, [refreshEnv]);

  useEffect(() => {
    if (!api || !qrcode || env?.connected) return;

    let cancelled = false;
    const poll = async () => {
      try {
        const result = await api.pollLogin(qrcode.token);
        if (cancelled) return;
        const state = result.state;
        if (state === 'connected') {
          setQrcode(null);
          setErrorMessage(null);
          toast(t(locale, 'wechat.connected'), 'success');
          await refreshEnv(false);
          return;
        }
        if (state === 'expired') {
          setQrcode(null);
          setEnv(prev => prev ? { ...prev, state: 'expired', connected: false } : prev);
          setErrorMessage(result.error || t(locale, 'wechat.qrExpired'));
          return;
        }
        if (state === 'canceled') {
          setQrcode(null);
          setEnv(prev => prev ? { ...prev, state: 'canceled', connected: false } : prev);
          return;
        }
        setEnv(prev => prev ? { ...prev, state, connected: false } : prev);
      } catch (error) {
        if (!cancelled) showError('wechat.pollFailed', error);
      }
    };

    poll();
    const interval = window.setInterval(poll, 5_000);
    // T218 (P2-004): pause polling while the tab is hidden (visibility-gated).
    const onVisibility = () => {
      if (document.hidden) window.clearInterval(interval);
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [api, env?.connected, locale, qrcode, refreshEnv, showError, toast]);

  const handleLogin = useCallback(async () => {
    if (!api) return;
    setLoginLoading(true);
    setErrorMessage(null);
    try {
      const r = await api.login();
      if (!mountedRef.current) return;
      if (r.qrcode && r.qrcode_img_content) {
        setQrcode({ token: r.qrcode, image: r.qrcode_img_content });
        setEnv(prev => prev ? { ...prev, state: 'qr_showing', connected: false } : prev);
      } else {
        throw new Error('QR code response is incomplete');
      }
    } catch (error) {
      showError('wechat.loginFailed', error);
    } finally {
      if (mountedRef.current) setLoginLoading(false);
    }
  }, [api, showError]);

  const handleDisconnect = useCallback(async () => {
    if (!api) return;
    setDisconnecting(true);
    setErrorMessage(null);
    try {
      await api.disconnect();
      if (!mountedRef.current) return;
      setQrcode(null);
      setMessages([]);
      await refreshEnv(false);
      toast(t(locale, 'wechat.disconnected'), 'success');
    } catch (error) {
      showError('wechat.disconnectFailed', error);
    } finally {
      if (mountedRef.current) setDisconnecting(false);
    }
  }, [api, locale, refreshEnv, showError, toast]);

  const handleSend = useCallback(async () => {
    const text = message.trim();
    if (!api || !text || sending) return;
    setSending(true);
    setErrorMessage(null);
    try {
      await api.send(text);
      if (!mountedRef.current) return;
      setMessages(prev => [...prev, { role: 'user', text }]);
      setMessage('');
      toast(t(locale, 'wechat.sent'), 'success');
    } catch (error) {
      showError('wechat.sendFailed', error);
    } finally {
      if (mountedRef.current) setSending(false);
    }
  }, [api, locale, message, sending, showError, toast]);

  if (envLoading || !env) {
    return (
      <div className="flex flex-col items-center justify-center gap-3 p-8">
        {envLoading && <RefreshCw className="animate-spin" size={20} />}
        {errorMessage && (
          <>
            <p className="text-sm text-center" style={{ color: 'var(--danger)' }}>{errorMessage}</p>
            <button onClick={() => refreshEnv(true)} className="px-3 py-1.5 rounded text-xs" style={{ background: 'var(--surface)', color: 'var(--text)' }}>
              {t(locale, 'wechat.retry')}
            </button>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between p-3 border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="flex items-center gap-2">
          {env.connected ? (
            <Wifi size={16} style={{ color: 'var(--primary)' }} />
          ) : (
            <WifiOff size={16} style={{ color: 'var(--text-disabled)' }} />
          )}
          <span className="text-sm font-medium">{t(locale, 'wechat.title')}</span>
          <span className="text-xs px-1.5 py-0.5 rounded" style={{
            background: env.connected ? 'var(--primary-soft)' : 'var(--surface)',
            color: env.connected ? 'var(--primary)' : 'var(--text-disabled)'
          }}>
            {stateLabel(locale, env.state)}
          </span>
        </div>
        <button onClick={onClose} className="text-xs" style={{ color: 'var(--text-secondary)' }}>{t(locale, 'wechat.close')}</button>
      </div>

      <div className="flex-1 overflow-auto p-4">
        {errorMessage && (
          <div className="mb-3 rounded border px-3 py-2 text-xs" style={{ borderColor: 'var(--danger)', color: 'var(--danger)', background: 'var(--surface)' }}>
            {errorMessage}
          </div>
        )}

        {!env.connected && !qrcode && (
          <div className="flex flex-col items-center gap-4 py-8">
            <QrCode size={48} style={{ color: 'var(--text-disabled)' }} />
            <p className="text-sm text-center" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'wechat.scanHint')}
            </p>
            <button
              onClick={handleLogin}
              disabled={loginLoading}
              className="px-4 py-2 rounded text-sm"
              style={{ background: 'var(--primary)', color: 'var(--accent-ink)', opacity: loginLoading ? 0.7 : 1 }}
            >
              {loginLoading ? t(locale, 'wechat.loading') : t(locale, 'wechat.getQrcode')}
            </button>
          </div>
        )}

        {qrcode && !env.connected && (
          <div className="flex flex-col items-center gap-4 py-4">
            <img src={qrcode.image} alt="QR Code" className="w-48 h-48" />
            <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>{t(locale, 'wechat.scanToLogin')}</p>
            <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>{t(locale, 'wechat.waitingConfirm')}</p>
          </div>
        )}

        {env.connected && (
          <div className="flex flex-col gap-3">
            {messages.map((m, i) => (
              <div key={i} className={`text-sm p-2 rounded ${m.role === 'user' ? 'ml-8' : 'mr-8'}`} style={{
                background: m.role === 'user' ? 'var(--primary-soft)' : 'var(--surface)',
                color: m.role === 'user' ? 'var(--primary)' : 'var(--text)'
              }}>
                {m.text}
              </div>
            ))}
          </div>
        )}
      </div>

      {env.connected && (
        <div className="flex items-center gap-2 p-3 border-t" style={{ borderColor: 'var(--border)' }}>
          <input
            type="text"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && !sending) handleSend(); }}
            placeholder={t(locale, 'wechat.messagePlaceholder')}
            disabled={sending}
            className="flex-1 bg-transparent border rounded px-2 py-1 text-sm"
            style={{ borderColor: 'var(--border)', color: 'var(--text)' }}
          />
          <button
            onClick={handleSend}
            disabled={sending || !message.trim()}
            className="p-1.5 rounded"
            style={{ background: 'var(--primary)', color: 'var(--accent-ink)', opacity: sending || !message.trim() ? 0.6 : 1 }}
            title={sending ? t(locale, 'wechat.sending') : undefined}
          >
            <Send size={14} />
          </button>
          <button
            onClick={handleDisconnect}
            disabled={disconnecting}
            className="p-1.5 rounded"
            style={{ background: 'var(--surface)', color: 'var(--text-secondary)', opacity: disconnecting ? 0.6 : 1 }}
            title={t(locale, 'wechat.disconnect')}
          >
            {disconnecting ? <RefreshCw className="animate-spin" size={14} /> : <WifiOff size={14} />}
          </button>
        </div>
      )}
    </div>
  );
}
