'use client';

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { t } from '@/i18n';

interface ModelInfo {
  id: string;
  displayName?: string;
  contextWindow?: number;
  maxOutput?: number;
  capabilities?: {
    streaming?: boolean;
    toolCalling?: boolean;
    imageInput?: boolean;
    reasoning?: boolean;
  };
  source?: 'api_discovery' | 'cache' | 'preset' | 'manual';
  discoveredAt?: string;
}

export interface ProviderWithModels {
  id: string;
  name: string;
  presetName: string;
  baseUrl: string;
  keys: Array<{
    id: string;
    label: string;
    maskedKey: string;
    isActive?: boolean;
    status?: 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable' | string;
  }>;
  models?: ModelInfo[];
  defaultModel?: string | null;
}

interface ModelSelectorDropdownProps {
  providers: ProviderWithModels[];
  selectedProviderId: string;
  selectedModel?: string;
  onSelect: (providerId: string, model: string) => void;
  locale: string;
}

export default function ModelSelectorDropdown({
  providers,
  selectedProviderId,
  selectedModel,
  onSelect,
  locale,
}: ModelSelectorDropdownProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number; width: number } | null>(
    null,
  );
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const selectedProvider =
    providers.find((p) => p.id === selectedProviderId) ??
    providers.find((p) => (p.models ?? []).some((m) => m.id === selectedModel)) ??
    providers[0];
  const models = selectedProvider?.models?.map((m: ModelInfo) => m.id) ?? [];
  const selectedModelInfo = selectedProvider?.models?.find((m) => m.id === selectedModel);
  const selectedLabel =
    selectedModelInfo?.displayName && selectedModelInfo.displayName !== selectedModel
      ? selectedModelInfo.displayName
      : selectedModel || models[0] || t(locale, 'modelSelector.noModel');

  // Position menu above the trigger so overflow:hidden parents cannot clip it.
  useLayoutEffect(() => {
    if (!isOpen || !triggerRef.current) {
      setMenuPos(null);
      return;
    }
    const rect = triggerRef.current.getBoundingClientRect();
    setMenuPos({
      top: rect.top,
      left: rect.left,
      width: Math.max(rect.width, 220),
    });
  }, [isOpen, providers.length]);

  // Click outside to close (trigger + portal menu).
  useEffect(() => {
    if (!isOpen) return;
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      setIsOpen(false);
    };
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setIsOpen(false);
    };
    document.addEventListener('mousedown', handleClick);
    document.addEventListener('keydown', handleKey);
    return () => {
      document.removeEventListener('mousedown', handleClick);
      document.removeEventListener('keydown', handleKey);
    };
  }, [isOpen]);

  // Keep menu aligned on scroll/resize while open.
  useEffect(() => {
    if (!isOpen) return;
    const reposition = () => {
      if (!triggerRef.current) return;
      const rect = triggerRef.current.getBoundingClientRect();
      setMenuPos({
        top: rect.top,
        left: rect.left,
        width: Math.max(rect.width, 220),
      });
    };
    window.addEventListener('resize', reposition);
    // capture so nested scroll containers also update position
    window.addEventListener('scroll', reposition, true);
    return () => {
      window.removeEventListener('resize', reposition);
      window.removeEventListener('scroll', reposition, true);
    };
  }, [isOpen]);

  const label = selectedProvider
    ? `${selectedProvider.name} / ${selectedLabel}`
    : t(locale, 'modelSelector.selectModel');

  const menu =
    isOpen && menuPos && typeof document !== 'undefined'
      ? createPortal(
          <div
            ref={menuRef}
            role="listbox"
            aria-label={t(locale, 'modelSelector.selectModel')}
            className="fixed z-[200] max-h-72 min-w-[220px] overflow-y-auto rounded-xl border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-popup"
            style={{
              top: menuPos.top,
              left: menuPos.left,
              width: menuPos.width,
              transform: 'translateY(calc(-100% - 6px))',
            }}
          >
            {providers.length === 0 ? (
              <div className="px-2.5 py-3 text-center text-[0.6875rem] text-[var(--text-disabled)]">
                {t(locale, 'modelSelector.noProviders')}
              </div>
            ) : (
              providers.map((provider) => {
                const providerModels = provider.models ?? [];
                return (
                  <div key={provider.id}>
                    <div className="px-2.5 pb-1 pt-1.5 text-[0.625rem] font-semibold uppercase tracking-[0.06em] text-[var(--text-disabled)]">
                      {provider.name}
                    </div>
                    {providerModels.length === 0 ? (
                      <div className="px-2.5 py-2 text-xs text-[var(--text-disabled)]">
                        {t(locale, 'modelSelector.noModels')}
                      </div>
                    ) : (
                      providerModels.map((modelInfo) => {
                        const model = modelInfo.id;
                        const itemLabel =
                          modelInfo.displayName && modelInfo.displayName !== model
                            ? modelInfo.displayName
                            : model;
                        const isSelected =
                          selectedProviderId === provider.id && selectedModel === model;
                        return (
                          <button
                            key={`${provider.id}:${model}`}
                            type="button"
                            role="option"
                            aria-selected={isSelected}
                            onClick={(e) => {
                              e.preventDefault();
                              e.stopPropagation();
                              onSelect(provider.id, model);
                              setIsOpen(false);
                            }}
                            className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm transition-all ${
                              isSelected
                                ? 'bg-[var(--accent)] font-medium text-[var(--accent-ink)]'
                                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
                            }`}
                          >
                            <span className="min-w-0 flex-1 truncate font-mono text-xs" title={model}>
                              {itemLabel}
                            </span>
                          </button>
                        );
                      })
                    )}
                  </div>
                );
              })
            )}
          </div>,
          document.body,
        )
      : null;

  return (
    <div className="relative">
      <button
        ref={triggerRef}
        type="button"
        onClick={() => setIsOpen((open) => !open)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        className="flex items-center gap-1.5 rounded-lg px-2 py-1 text-[0.6875rem] text-[var(--text-secondary)] transition-all hover:bg-[var(--surface-hover)]"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="10" />
          <path d="M2 12h20" />
          <path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z" />
        </svg>
        <span className="max-w-[160px] truncate" title={label}>
          {label}
        </span>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {menu}
    </div>
  );
}
