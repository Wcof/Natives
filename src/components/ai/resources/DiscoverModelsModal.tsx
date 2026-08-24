'use client';

import { useState } from 'react';
import { aiApi, type DiscoveredModel } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { Sparkles, X, RefreshCw } from 'lucide-react';

interface DiscoverModelsModalProps {
  providerId: string;
  connectionId?: string;
  defaultBaseUrl: string;
  onClose: () => void;
  onSuccess: () => void;
}

export function DiscoverModelsModal({
  providerId,
  connectionId,
  defaultBaseUrl,
  onClose,
  onSuccess,
}: DiscoverModelsModalProps) {
  const locale = useLocale();
  const [baseUrl, setBaseUrl] = useState(defaultBaseUrl);
  const [apiKey, setApiKey] = useState('');
  const [discoveredModels, setDiscoveredModels] = useState<DiscoveredModel[]>([]);
  const [selectedDiscovered, setSelectedDiscovered] = useState<Record<string, boolean>>({});
  const [discovering, setDiscovering] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleStartDiscover = async () => {
    if (!baseUrl.trim()) return;
    setDiscovering(true);
    setError(null);
    try {
      const res = await aiApi.discoverModels({
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
      });
      setDiscoveredModels(res);
      const sel: Record<string, boolean> = {};
      for (const m of res) {
        sel[m.id] = true;
      }
      setSelectedDiscovered(sel);
    } catch (err) {
      setError(String(err));
    } finally {
      setDiscovering(false);
    }
  };

  const handleConfirmDiscovered = async () => {
    const toSave = discoveredModels.filter((m) => selectedDiscovered[m.id]);
    setError(null);
    try {
      await aiApi.confirmDiscoveredModels({
        providerId,
        connectionId,
        models: toSave,
      });
      onSuccess();
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-xl p-6 shadow-2xl space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="font-bold text-lg flex items-center gap-2">
            <Sparkles className="w-5 h-5 text-[var(--primary)]" />
            {t(locale, 'aiResources.discoverModels')}
          </h3>
          <button onClick={onClose} className="p-1 rounded text-[var(--muted-foreground)] hover:text-[var(--foreground)]">
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="space-y-3">
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">Endpoint Base URL</label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              className="w-full mt-1 px-3 py-1.5 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm font-mono"
            />
          </div>
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">API Key (Optional for discovery)</label>
            <input
              type="password"
              placeholder="sk-..."
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="w-full mt-1 px-3 py-1.5 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm font-mono"
            />
          </div>
          <button
            onClick={handleStartDiscover}
            disabled={discovering}
            className="w-full py-2 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-sm font-medium flex items-center justify-center gap-2"
          >
            {discovering && <RefreshCw className="w-4 h-4 animate-spin" />}
            {discovering ? t(locale, 'aiResources.discovering') : t(locale, 'aiResources.startFetch')}
          </button>

          {error && (
            <div className="p-3 rounded-lg bg-[var(--destructive)]/10 text-[var(--destructive)] text-xs">
              {error}
            </div>
          )}

          {discoveredModels.length > 0 && (
            <div className="space-y-2 mt-4">
              <div className="flex justify-between text-xs text-[var(--muted-foreground)]">
                <span>Discovered {discoveredModels.length} models</span>
                <button
                  onClick={() => {
                    const allSelected = Object.values(selectedDiscovered).every(Boolean);
                    const next: Record<string, boolean> = {};
                    for (const m of discoveredModels) {
                      next[m.id] = !allSelected;
                    }
                    setSelectedDiscovered(next);
                  }}
                  className="text-[var(--primary)] hover:underline"
                >
                  Toggle All
                </button>
              </div>
              <div className="max-h-60 overflow-y-auto space-y-1.5 border border-[var(--border)] p-2 rounded-xl">
                {discoveredModels.map((m) => (
                  <label
                    key={m.id}
                    className="flex items-center gap-2 p-2 rounded-lg hover:bg-[var(--secondary)] cursor-pointer text-xs"
                  >
                    <input
                      type="checkbox"
                      checked={!!selectedDiscovered[m.id]}
                      onChange={(e) =>
                        setSelectedDiscovered((prev) => ({ ...prev, [m.id]: e.target.checked }))
                      }
                      className="rounded border-[var(--border)]"
                    />
                    <span className="font-mono font-medium">{m.id}</span>
                    {m.displayName !== m.id && (
                      <span className="text-[var(--muted-foreground)]">({m.displayName})</span>
                    )}
                  </label>
                ))}
              </div>
              <div className="flex justify-end gap-2 pt-2">
                <button
                  onClick={handleConfirmDiscovered}
                  className="px-4 py-2 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-sm font-medium shadow-sm"
                >
                  {t(locale, 'aiResources.saveSelectedModels')}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
