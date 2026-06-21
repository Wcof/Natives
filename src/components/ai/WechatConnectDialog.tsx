'use client';

import { useEffect, useState, useCallback } from 'react';
import { QrCode, Send, Wifi, WifiOff, RefreshCw } from 'lucide-react';

interface WechatConnectDialogProps {
  onClose: () => void;
}

type ConnState = 'uninstalled' | 'installed_off' | 'gateway_online' | 'selecting_agent' | 'qr_generated' | 'qr_showing' | 'connected' | 'expired';

interface BridgeEnv {
  target: string;
  cwd: string;
  persona: string;
  state: ConnState;
  connected: boolean;
}

// Use the global nativesAPI exposed by tauri-adapter (cast to any to access optional wechat namespace)
const api = (typeof window !== 'undefined' ? (window as any).nativesAPI?.wechat : undefined) as
  | {
      env: () => Promise<BridgeEnv>;
      login: () => Promise<{ qrcode: string; qrcode_img_content: string; state: string }>;
      disconnect: () => Promise<{ ok: boolean }>;
      check: () => Promise<{ ok: boolean; state: string }>;
      send: (text: string) => Promise<{ ok: boolean; cid: string }>;
      setTarget: (target: string) => Promise<void>;
      setCwd: (dir: string) => Promise<void>;
      setPersona: (persona: string) => Promise<void>;
      detectAgents: () => Promise<{ claude: boolean; codex: boolean }>;
      status: () => Promise<{ state: string; connected: boolean; target: string; cwd: string }>;
    }
  | undefined;

export default function WechatConnectDialog({ onClose }: WechatConnectDialogProps) {
  const [env, setEnv] = useState<BridgeEnv | null>(null);
  const [qrcode, setQrcode] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState('');
  const [messages, setMessages] = useState<Array<{ role: string; text: string }>>([]);

  const refreshEnv = useCallback(async () => {
    if (!api) return;
    try {
      const e = await api.env();
      setEnv(e);
    } catch (err) {
      console.error('Failed to fetch wechat env:', err);
    }
  }, []);

  useEffect(() => {
    refreshEnv();
    const interval = setInterval(refreshEnv, 2000);
    return () => clearInterval(interval);
  }, [refreshEnv]);

  const handleLogin = useCallback(async () => {
    if (!api) return;
    setLoading(true);
    try {
      const r = await api.login();
      if (r.qrcode_img_content) {
        setQrcode(r.qrcode_img_content);
      }
    } catch (err) {
      console.error('Login failed:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleDisconnect = useCallback(async () => {
    if (!api) return;
    await api.disconnect();
    setQrcode(null);
    refreshEnv();
  }, [refreshEnv]);

  const handleSend = useCallback(async () => {
    if (!api || !message.trim()) return;
    try {
      await api.send(message);
      setMessages(prev => [...prev, { role: 'user', text: message }]);
      setMessage('');
    } catch (err) {
      console.error('Send failed:', err);
    }
  }, [message]);

  if (!env) {
    return (
      <div className="flex items-center justify-center p-8">
        <RefreshCw className="animate-spin" size={20} />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between p-3 border-b" style={{ borderColor: 'var(--border)' }}>
        <div className="flex items-center gap-2">
          {env.connected ? (
            <Wifi size={16} style={{ color: 'var(--accent)' }} />
          ) : (
            <WifiOff size={16} style={{ color: 'var(--text-faint)' }} />
          )}
          <span className="text-sm font-medium">微信 ClawBot</span>
          <span className="text-xs px-1.5 py-0.5 rounded" style={{
            background: env.connected ? 'var(--accent-soft)' : 'var(--vibe-btn-bg)',
            color: env.connected ? 'var(--accent)' : 'var(--text-faint)'
          }}>
            {env.state}
          </span>
        </div>
        <button onClick={onClose} className="text-xs" style={{ color: 'var(--text-dim)' }}>关闭</button>
      </div>

      <div className="flex-1 overflow-auto p-4">
        {!env.connected && !qrcode && (
          <div className="flex flex-col items-center gap-4 py-8">
            <QrCode size={48} style={{ color: 'var(--text-faint)' }} />
            <p className="text-sm text-center" style={{ color: 'var(--text-dim)' }}>
              扫码登录微信，遥控本机的 Claude Code / Codex
            </p>
            <button
              onClick={handleLogin}
              disabled={loading}
              className="px-4 py-2 rounded text-sm"
              style={{ background: 'var(--accent)', color: 'var(--accent-ink)' }}
            >
              {loading ? '加载中…' : '获取二维码'}
            </button>
          </div>
        )}

        {qrcode && !env.connected && (
          <div className="flex flex-col items-center gap-4 py-4">
            <img src={qrcode} alt="QR Code" className="w-48 h-48" />
            <p className="text-xs" style={{ color: 'var(--text-faint)' }}>用微信扫码登录</p>
          </div>
        )}

        {env.connected && (
          <div className="flex flex-col gap-3">
            {messages.map((m, i) => (
              <div key={i} className={`text-sm p-2 rounded ${m.role === 'user' ? 'ml-8' : 'mr-8'}`} style={{
                background: m.role === 'user' ? 'var(--accent-soft)' : 'var(--vibe-btn-bg)',
                color: m.role === 'user' ? 'var(--accent)' : 'var(--text)'
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
            onKeyDown={(e) => { if (e.key === 'Enter') handleSend(); }}
            placeholder="发送消息给本机 agent…"
            className="flex-1 bg-transparent border rounded px-2 py-1 text-sm"
            style={{ borderColor: 'var(--border)', color: 'var(--text)' }}
          />
          <button
            onClick={handleSend}
            className="p-1.5 rounded"
            style={{ background: 'var(--accent)', color: 'var(--accent-ink)' }}
          >
            <Send size={14} />
          </button>
          <button
            onClick={handleDisconnect}
            className="p-1.5 rounded"
            style={{ background: 'var(--vibe-btn-bg)', color: 'var(--text-dim)' }}
          >
            <WifiOff size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
