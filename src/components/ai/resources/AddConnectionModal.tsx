'use client';

import { useState } from 'react';
import { aiApi, type UpstreamProtocol } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { X } from 'lucide-react';

interface AddConnectionModalProps {
  providerId: string;
  defaultName: string;
  defaultBaseUrl: string;
  onClose: () => void;
  onSuccess: () => void;
}

export function AddConnectionModal({
  providerId,
  defaultName,
  defaultBaseUrl,
  onClose,
  onSuccess,
}: AddConnectionModalProps) {
  const locale = useLocale();
  const [form, setForm] = useState({
    name: defaultName,
    baseUrl: defaultBaseUrl,
    upstreamProtocol: 'openai_chat_completions' as UpstreamProtocol,
    modelsUrl: '',
  });
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!form.name.trim() || !form.baseUrl.trim()) return;
    setError(null);
    try {
      await aiApi.createConnection({
        providerId,
        name: form.name.trim(),
        baseUrl: form.baseUrl.trim(),
        upstreamProtocol: form.upstreamProtocol,
        modelsUrl: form.modelsUrl.trim() || undefined,
        enabled: true,
      });
      onSuccess();
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-md p-6 shadow-2xl space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="font-bold text-lg">{t(locale, 'aiResources.addConnectionModal')}</h3>
          <button onClick={onClose} className="p-1 rounded text-[var(--muted-foreground)] hover:text-[var(--foreground)]">
            <X className="w-5 h-5" />
          </button>
        </div>
        {error && (
          <div className="p-3 rounded-lg bg-[var(--destructive)]/10 text-[var(--destructive)] text-xs">{error}</div>
        )}
        <form onSubmit={handleSubmit} className="space-y-3">
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">
              {t(locale, 'aiResources.connectionName')}
            </label>
            <input
              type="text"
              required
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
            />
          </div>
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">Base URL</label>
            <input
              type="url"
              required
              placeholder="https://api.openai.com/v1"
              value={form.baseUrl}
              onChange={(e) => setForm((f) => ({ ...f, baseUrl: e.target.value }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm font-mono focus:outline-none focus:border-[var(--primary)]"
            />
          </div>
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">
              {t(locale, 'aiResources.upstreamProtocol')}
            </label>
            <select
              value={form.upstreamProtocol}
              onChange={(e) => setForm((f) => ({ ...f, upstreamProtocol: e.target.value as UpstreamProtocol }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
            >
              <option value="openai_chat_completions">OpenAI Chat Completions</option>
              <option value="openai_responses">OpenAI Responses</option>
              <option value="anthropic_messages">Anthropic Messages</option>
            </select>
          </div>
          <div className="flex justify-end gap-2 pt-3">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 bg-[var(--secondary)] text-[var(--secondary-foreground)] rounded-lg text-sm font-medium"
            >
              {t(locale, 'aiResources.cancel')}
            </button>
            <button
              type="submit"
              className="px-4 py-2 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-sm font-medium shadow-sm"
            >
              {t(locale, 'aiResources.save')}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
