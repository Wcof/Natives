'use client';

import { useState, useRef, useEffect } from 'react';

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
  keys: Array<{ id: string; label: string; maskedKey: string }>;
  models?: ModelInfo[];
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
  const dropdownRef = useRef<HTMLDivElement>(null);

  const selectedProvider = providers.find((p) => p.id === selectedProviderId);
  const models = selectedProvider?.models?.map((m: ModelInfo) => m.id) ?? [];

  // Click outside to close
  useEffect(() => {
    if (!isOpen) return;
    const handleClick = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setIsOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [isOpen]);

  const label = selectedProvider
    ? `${selectedProvider.name} / ${selectedModel || models[0] || (locale.startsWith('zh') ? '未选择模型' : 'No model')}`
    : locale.startsWith('zh') ? '选择模型' : 'Select model';

  return (
    <div ref={dropdownRef} className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-1.5 px-2 py-1 rounded-lg text-[0.6875rem] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] transition-all"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="10" />
          <path d="M2 12h20" />
          <path d="M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z" />
        </svg>
        <span className="truncate max-w-[120px]">{label}</span>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {isOpen && (
        <div className="absolute bottom-full left-0 mb-1 min-w-[200px] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-popup z-50">
          {providers.length === 0 ? (
            <div className="px-2.5 py-3 text-[0.6875rem] text-[var(--text-disabled)] text-center">
              {locale.startsWith('zh') ? '暂无可用供应商' : 'No providers available'}
            </div>
          ) : (
            providers.map((provider) => {
              const providerModels = provider.models?.map((m: ModelInfo) => m.id) ?? [];
              return (
                <div key={provider.id}>
                  <div className="px-2.5 pb-1 pt-1.5 text-[0.625rem] font-semibold uppercase tracking-[0.06em] text-[var(--text-disabled)]">
                    {provider.name}
                  </div>
                  {providerModels.length === 0 ? (
                    <div className="px-2.5 py-2 text-xs text-[var(--text-disabled)]">
                      {locale.startsWith('zh') ? '未发现可用模型' : 'No discovered models'}
                    </div>
                  ) : (provider.models ?? []).map((modelInfo) => {
                    const model = modelInfo.id;
                    const label =
                      modelInfo.displayName && modelInfo.displayName !== model
                        ? modelInfo.displayName
                        : model;
                    return (
                    <button
                      key={`${provider.id}:${model}`}
                      type="button"
                      onClick={() => {
                        onSelect(provider.id, model);
                        setIsOpen(false);
                      }}
                      className={`flex items-center gap-2 w-full rounded-lg px-2.5 py-1.5 text-left text-sm transition-all ${
                        selectedProviderId === provider.id && selectedModel === model
                          ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                          : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
                      }`}
                    >
                      <span className="min-w-0 flex-1 truncate text-xs font-mono" title={model}>
                        {label}
                      </span>
                    </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
