'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { Check, Download, Globe, Search } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { installMcpHubEntry, searchMcpHub } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import { useToast } from '@/components/ui/Toast';
import type { McpHubEntry } from '../shared/capability-types';

interface McpHubBrowserProps {
  locale: Locale;
  gateway: AssistantGateway;
  onInstalled: () => void;
  /** Hub unreachable → guide the user to offline JSON import. */
  onFallbackToJson: () => void;
}

const SEARCH_DEBOUNCE_MS = 300;
const PAGE_SIZE = 20;

/**
 * Read-only browse of the official MCP registry (ADR-0016 决策 6):
 * explicit install only, artifacts land untrusted + disabled, honest errors.
 */
export default function McpHubBrowser({ locale, gateway, onInstalled, onFallbackToJson }: McpHubBrowserProps) {
  const { toast } = useToast();
  const [query, setQuery] = useState('');
  const [entries, setEntries] = useState<McpHubEntry[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [phase, setPhase] = useState<'loading' | 'error' | 'ready'>('loading');
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestSeq = useRef(0);

  const runSearch = useCallback(
    async (q: string, cursor: string | null) => {
      const seq = ++requestSeq.current;
      if (cursor) setLoadingMore(true);
      else {
        setPhase('loading');
        setError(null);
      }
      try {
        const result = await searchMcpHub(gateway, {
          ...(q.trim() ? { query: q.trim() } : {}),
          ...(cursor ? { cursor } : {}),
          limit: PAGE_SIZE,
        });
        if (seq !== requestSeq.current) return;
        setEntries((prev) => (cursor ? [...prev, ...result.servers] : result.servers));
        setNextCursor(result.nextCursor);
        setStale(result.stale);
        setPhase('ready');
      } catch (e) {
        if (seq !== requestSeq.current) return;
        setError(classifyError(e).userMessage);
        if (!cursor) setPhase('error');
      } finally {
        if (seq === requestSeq.current) setLoadingMore(false);
      }
    },
    [gateway],
  );

  // Initial load + debounced re-search (300ms).
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => void runSearch(query, null), query ? SEARCH_DEBOUNCE_MS : 0);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, runSearch]);

  const handleInstall = useCallback(
    async (entry: McpHubEntry) => {
      setInstalling(entry.registryName);
      try {
        await installMcpHubEntry(gateway, { registryName: entry.registryName });
        toast(t(locale, 'capabilities.hub.installedToast'), 'success');
        setEntries((prev) =>
          prev.map((item) => (item.registryName === entry.registryName ? { ...item, installed: true } : item)),
        );
        onInstalled();
      } catch (e) {
        toast(classifyError(e).userMessage, 'error');
      } finally {
        setInstalling(null);
      }
    },
    [gateway, locale, onInstalled, toast],
  );

  return (
    <div className="flex h-full flex-col gap-3">
      <div
        className="flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5"
        style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
      >
        <Search size={14} style={{ color: 'var(--text-disabled)' }} aria-hidden />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t(locale, 'capabilities.hub.searchPlaceholder')}
          aria-label={t(locale, 'capabilities.hub.searchPlaceholder')}
          className="w-full bg-transparent text-sm"
          style={{ color: 'var(--text)', outline: 'none', border: 'none' }}
        />
      </div>

      <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>
        {t(locale, 'capabilities.hub.installNote')}
      </p>

      {stale ? (
        <p className="rounded-lg border px-2.5 py-1.5 text-xs" style={{ borderColor: 'var(--warning)', color: 'var(--warning)' }}>
          {t(locale, 'capabilities.hub.stale')}
        </p>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {phase === 'loading' ? (
          <LoadingState message={t(locale, 'capabilities.common.loading')} />
        ) : phase === 'error' ? (
          <div>
            {/* Honest unreachable state + offline JSON import guidance (no fake cache). */}
            <ErrorState
              message={`${t(locale, 'capabilities.hub.unreachable')}${error ? ` — ${error}` : ''}`}
              onRetry={() => void runSearch(query, null)}
            />
            <div className="flex justify-center">
              <button
                type="button"
                onClick={onFallbackToJson}
                className="rounded-lg border px-3 py-1.5 text-sm"
                style={{ borderColor: 'var(--border-subtle)', color: 'var(--primary)' }}
              >
                {t(locale, 'capabilities.hub.fallbackToJson')}
              </button>
            </div>
          </div>
        ) : entries.length === 0 ? (
          <EmptyState title={t(locale, 'capabilities.hub.empty')} />
        ) : (
          <div className="flex flex-col gap-1">
            {entries.map((entry) => (
              <div
                key={entry.registryName}
                className="flex items-center gap-2.5 rounded-lg border px-3 py-2"
                style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
              >
                <Globe size={16} className="shrink-0" style={{ color: 'var(--primary)' }} aria-hidden />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium" style={{ color: 'var(--text)' }}>
                      {entry.name || entry.registryName}
                    </span>
                    {entry.version ? (
                      <span className="font-mono text-[10px]" style={{ color: 'var(--text-disabled)' }}>
                        v{entry.version}
                      </span>
                    ) : null}
                    {entry.installed ? (
                      <span
                        className="flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
                        style={{ borderColor: 'var(--border-subtle)', color: 'var(--success, #10b981)' }}
                      >
                        <Check size={10} aria-hidden />
                        {t(locale, 'capabilities.hub.installed')}
                      </span>
                    ) : null}
                  </div>
                  {entry.description ? (
                    <p className="mt-0.5 truncate text-xs" style={{ color: 'var(--text-secondary)' }}>
                      {entry.description}
                    </p>
                  ) : null}
                </div>
                {!entry.installed ? (
                  <button
                    type="button"
                    onClick={() => void handleInstall(entry)}
                    disabled={installing !== null}
                    className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm disabled:opacity-50"
                    style={{ borderColor: 'var(--primary)', color: 'var(--primary)' }}
                  >
                    <Download size={13} aria-hidden />
                    {installing === entry.registryName
                      ? t(locale, 'capabilities.hub.installing')
                      : t(locale, 'capabilities.hub.install')}
                  </button>
                ) : null}
              </div>
            ))}
            {nextCursor ? (
              <button
                type="button"
                onClick={() => void runSearch(query, nextCursor)}
                disabled={loadingMore}
                className="mt-1 rounded-lg border px-3 py-2 text-sm disabled:opacity-50"
                style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
              >
                {loadingMore ? t(locale, 'capabilities.common.loading') : t(locale, 'capabilities.hub.loadMore')}
              </button>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}
