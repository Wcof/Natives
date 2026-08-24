'use client';

import { useState } from 'react';
import { aiApi, type Provider } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { X } from 'lucide-react';

interface AddProviderModalProps {
  onClose: () => void;
  onSuccess: (p: Provider) => void;
}

export function AddProviderModal({ onClose, onSuccess }: AddProviderModalProps) {
  const locale = useLocale();
  const [form, setForm] = useState({ name: '', websiteUrl: '', presetKey: '' });
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!form.name.trim()) return;
    setError(null);
    try {
      const p = await aiApi.createProvider({
        name: form.name.trim(),
        websiteUrl: form.websiteUrl.trim(),
        presetKey: form.presetKey.trim() || undefined,
        enabled: true,
      });
      onSuccess(p);
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-md p-6 shadow-2xl space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="font-bold text-lg">{t(locale, 'aiResources.addProviderModal')}</h3>
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
              {t(locale, 'aiResources.providerName')}
            </label>
            <input
              type="text"
              required
              placeholder="e.g. DeepSeek, OpenRouter"
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
            />
          </div>
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">
              {t(locale, 'aiResources.websiteUrl')}
            </label>
            <input
              type="url"
              placeholder="https://..."
              value={form.websiteUrl}
              onChange={(e) => setForm((f) => ({ ...f, websiteUrl: e.target.value }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
            />
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
