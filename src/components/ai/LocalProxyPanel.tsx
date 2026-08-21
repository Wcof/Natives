'use client';

import { useState, useEffect, useCallback } from 'react';
import { proxy, type ProxyStatusResult, type ProxyChatResult } from '@/lib/tauri/proxy';
import { aiApi, type Provider, type Credential } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { Shield, Play, Square, Send, Server, Activity, CheckCircle2, AlertCircle, RefreshCw, Cpu } from 'lucide-react';

export default function LocalProxyPanel() {
  const locale = useLocale();
  const [status, setStatus] = useState<ProxyStatusResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [toggling, setToggling] = useState(false);

  // Test Console State
  const [providers, setProviders] = useState<Provider[]>([]);
  const [credentials, setCredentials] = useState<Record<string, Credential[]>>({});
  const [selectedProviderId, setSelectedProviderId] = useState('');
  const [protocol, setProtocol] = useState<'anthropic_messages' | 'openai_chat_completions' | 'openai_responses'>('openai_chat_completions');
  const [model, setModel] = useState('gpt-4o');
  const [testPrompt, setTestPrompt] = useState('Hello! Please reply in one short sentence.');
  const [sending, setSending] = useState(false);
  const [chatResult, setChatResult] = useState<ProxyChatResult | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const loadStatus = useCallback(async () => {
    setLoading(true);
    try {
      const s = await proxy.status();
      setStatus(s);
    } catch (e) {
      console.error('Failed to get proxy status:', e);
    } finally {
      setLoading(false);
    }
  }, []);

  const loadProviders = useCallback(async () => {
    try {
      const list = await aiApi.listProviders();
      setProviders(list);
      if (list.length > 0 && list[0]) {
        setSelectedProviderId(list[0].id);
        const credMap: Record<string, Credential[]> = {};
        for (const p of list) {
          const creds = await aiApi.listCredentials(p.id);
          credMap[p.id] = creds;
        }
        setCredentials(credMap);
      }
    } catch (e) {
      console.error('Failed to load providers for proxy test:', e);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
    void loadProviders();
  }, [loadStatus, loadProviders]);

  const handleToggle = async () => {
    if (!status) return;
    setToggling(true);
    try {
      if (status.running) {
        await proxy.stop();
      } else {
        await proxy.start();
      }
      await loadStatus();
    } catch (e) {
      console.error('Failed to toggle proxy:', e);
    } finally {
      setToggling(false);
    }
  };

  const handleSendTestChat = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedProviderId) {
      setErrorMsg('Provider not selected');
      return;
    }
    const p = providers.find((item) => item.id === selectedProviderId);
    if (!p) return;
    const creds = credentials[p.id] || [];
    const secretRef = creds.length > 0 && creds[0] ? creds[0].secretRef : `cred:provider:${p.id}:default`;

    setSending(true);
    setErrorMsg(null);
    setChatResult(null);

    try {
      const res = await proxy.chat({
        protocol,
        baseUrl: p.baseUrl || (protocol === 'anthropic_messages' ? 'https://api.anthropic.com' : 'https://api.openai.com/v1'),
        secretRef,
        model,
        requestJson: JSON.stringify({
          messages: [{ role: 'user', content: testPrompt }],
          model,
          stream: false,
        }),
      });
      setChatResult(res);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="flex flex-col gap-6 w-full max-w-5xl mx-auto p-4">
      {/* Header Info */}
      <div className="flex items-center justify-between border-b border-[var(--border-subtle)] pb-4">
        <div>
          <h2 className="text-lg font-semibold text-[var(--text)] flex items-center gap-2">
            <Server className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'localProxy.title')}
          </h2>
          <p className="text-xs text-[var(--text-secondary)] mt-1">
            {t(locale, 'localProxy.desc')}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadStatus()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs bg-[var(--surface-hover)] text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors"
        >
          <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          {t(locale, 'localProxy.refresh')}
        </button>
      </div>

      {/* Top Status Banner */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        {/* Status & Control */}
        <div className="p-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] flex flex-col justify-between gap-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-[var(--text-secondary)]">{t(locale, 'localProxy.engineStatus')}</span>
            <span
              className={`flex items-center gap-1 text-xs px-2 py-0.5 rounded-full font-medium ${
                status?.running
                  ? 'bg-[var(--success-soft)] text-[var(--success)]'
                  : 'bg-[var(--surface-hover)] text-[var(--text-disabled)]'
              }`}
            >
              {status?.running ? <CheckCircle2 className="w-3.5 h-3.5" /> : <AlertCircle className="w-3.5 h-3.5" />}
              {status?.running ? t(locale, 'localProxy.running') : t(locale, 'localProxy.stopped')}
            </span>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-bold font-mono text-[var(--text)]">
              {status?.host}:{status?.port || 11434}
            </span>
          </div>
          <button
            type="button"
            onClick={() => void handleToggle()}
            disabled={toggling}
            className={`flex items-center justify-center gap-2 py-2 px-4 rounded-lg text-xs font-medium transition-all ${
              status?.running
                ? 'bg-[var(--danger-soft)] text-[var(--danger)] hover:bg-[var(--danger-soft)]'
                : 'bg-[var(--primary)] text-[var(--primary-foreground)] hover:opacity-90'
            }`}
          >
            {status?.running ? <Square className="w-3.5 h-3.5 fill-current" /> : <Play className="w-3.5 h-3.5 fill-current" />}
            {status?.running ? t(locale, 'localProxy.stopEngine') : t(locale, 'localProxy.startEngine')}
          </button>
        </div>

        {/* Security / Keychain Invariant */}
        <div className="p-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] flex flex-col justify-between gap-2">
          <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
            <Shield className="w-4 h-4 text-[var(--success)]" />
            {t(locale, 'localProxy.securityNotice')}
          </div>
          <p className="text-xs text-[var(--text-secondary)] leading-relaxed">
            {t(locale, 'localProxy.securityDesc')}
          </p>
          <div className="flex items-center gap-1.5 text-[10px] font-mono text-[var(--success)]">
            <CheckCircle2 className="w-3 h-3" />
            {t(locale, 'localProxy.keychainProof')}
          </div>
        </div>

        {/* Protocols Matrix */}
        <div className="p-4 rounded-xl border border-[var(--border-subtle)] bg-[var(--surface)] flex flex-col gap-2">
          <div className="flex items-center gap-2 text-xs font-semibold text-[var(--text-secondary)]">
            <Activity className="w-4 h-4 text-[var(--primary)]" />
            {t(locale, 'localProxy.protocols')}
          </div>
          <div className="flex flex-col gap-1 text-xs">
            <div className="flex items-center justify-between py-1 border-b border-[var(--border-subtle)]">
              <span className="text-[var(--text)]">Anthropic Messages</span>
              <span className="text-[var(--success)] font-mono text-[10px]">Full (Stream/Tools)</span>
            </div>
            <div className="flex items-center justify-between py-1 border-b border-[var(--border-subtle)]">
              <span className="text-[var(--text)]">OpenAI Chat Completions</span>
              <span className="text-[var(--success)] font-mono text-[10px]">Full (Stream/Tools)</span>
            </div>
            <div className="flex items-center justify-between py-1">
              <span className="text-[var(--text)]">OpenAI Responses</span>
              <span className="text-[var(--success)] font-mono text-[10px]">Full (Reasoning)</span>
            </div>
          </div>
        </div>
      </div>

      {/* Interactive Host ProxyEngine Test Console */}
      <div className="flex flex-col gap-4 p-5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface)]">
        <h3 className="text-sm font-semibold text-[var(--text)] flex items-center gap-2">
          <Cpu className="w-4 h-4 text-[var(--primary)]" />
          {t(locale, 'localProxy.hostTest')}
        </h3>
        <p className="text-xs text-[var(--text-secondary)]">
          {t(locale, 'localProxy.hostTestDesc')}
        </p>

        <form onSubmit={(e) => void handleSendTestChat(e)} className="flex flex-col gap-4">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            <div className="flex flex-col gap-1">
              <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'localProxy.targetProvider')}</label>
              <select
                value={selectedProviderId}
                onChange={(e) => setSelectedProviderId(e.target.value)}
                className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none"
              >
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.apiProtocol})
                  </option>
                ))}
              </select>
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'localProxy.protocol')}</label>
              <select
                value={protocol}
                onChange={(e) => setProtocol(e.target.value as typeof protocol)}
                className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none"
              >
                <option value="openai_chat_completions">OpenAI Chat Completions</option>
                <option value="anthropic_messages">Anthropic Messages</option>
                <option value="openai_responses">OpenAI Responses</option>
              </select>
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'localProxy.modelId')}</label>
              <input
                type="text"
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder={t(locale, 'localProxy.modelPlaceholder')}
                className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] font-mono focus:outline-none"
              />
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-xs text-[var(--text-secondary)]">{t(locale, 'localProxy.userMessage')}</label>
            <textarea
              rows={2}
              value={testPrompt}
              onChange={(e) => setTestPrompt(e.target.value)}
              className="px-3 py-2 rounded-lg bg-[var(--surface-hover)] border border-[var(--border-subtle)] text-xs text-[var(--text)] focus:outline-none resize-none"
            />
          </div>

          <div className="flex items-center justify-end">
            <button
              type="submit"
              disabled={sending || !status?.running}
              className="flex items-center gap-2 px-5 py-2 rounded-lg bg-[var(--primary)] text-[var(--primary-foreground)] text-xs font-medium hover:opacity-90 transition-opacity disabled:opacity-50"
            >
              <Send className={`w-3.5 h-3.5 ${sending ? 'animate-pulse' : ''}`} />
              {sending ? t(locale, 'localProxy.calling') : t(locale, 'localProxy.sendRequest')}
            </button>
          </div>
        </form>

        {/* Results Area */}
        {chatResult && (
          <div className={`p-4 rounded-xl border text-xs flex flex-col gap-2 ${chatResult.ok ? 'border-[var(--success)] bg-[var(--success-soft)]' : 'border-[var(--danger)] bg-[var(--danger-soft)]'}`}>
            <div className="flex items-center justify-between font-semibold">
              <span className={chatResult.ok ? 'text-[var(--success)] flex items-center gap-1.5' : 'text-[var(--danger)] flex items-center gap-1.5'}>
                {chatResult.ok ? <CheckCircle2 className="w-4 h-4" /> : <AlertCircle className="w-4 h-4" />}
                {chatResult.ok ? t(locale, 'localProxy.success') : `${t(locale, 'localProxy.failed')}: ${chatResult.errorCategory || 'Error'}`}
              </span>
              <span className="text-[var(--text-secondary)] font-mono">
                Tokens: {chatResult.usageInputTokens} in / {chatResult.usageOutputTokens} out
              </span>
            </div>
            {chatResult.stopReason && (
              <span className="text-[var(--text-secondary)]">Stop Reason: {chatResult.stopReason}</span>
            )}
            {chatResult.errorMessage && (
              <span className="text-[var(--danger)] font-mono text-[11px]">{chatResult.errorMessage}</span>
            )}
          </div>
        )}

        {errorMsg && (
          <div className="p-4 rounded-xl border border-[var(--danger)] bg-[var(--danger-soft)] text-xs text-[var(--danger)] font-mono">
            {errorMsg}
          </div>
        )}
      </div>
    </div>
  );
}
