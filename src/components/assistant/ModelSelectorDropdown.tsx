'use client';

import { useState, useRef, useEffect } from 'react';

interface ModelSelectorDropdownProps {
  providers: Array<{
    id: string;
    name: string;
    presetName: string;
    baseUrl: string;
    keys: Array<{ id: string; label: string; maskedKey: string }>;
  }>;
  selectedProviderId: string;
  selectedModel?: string;
  onSelect: (providerId: string, model: string) => void;
  locale: string;
}

const PRESET_MODELS: Record<string, string[]> = {
  'openai': ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo', 'gpt-3.5-turbo'],
  'anthropic': ['claude-sonnet-4-20250514', 'claude-3-5-sonnet-latest', 'claude-3-opus-latest'],
  'ollama': ['llama3', 'mistral', 'codellama', 'qwen2'],
};

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
  const models = selectedProvider ? PRESET_MODELS[selectedProvider.presetName] || ['gpt-4o'] : [];

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
    ? `${selectedProvider.name} / ${selectedModel || models[0] || 'default'}`
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
              const providerModels = PRESET_MODELS[provider.presetName] || ['gpt-4o'];
              return (
                <div key={provider.id}>
                  <div className="px-2.5 pb-1 pt-1.5 text-[0.625rem] font-semibold uppercase tracking-[0.06em] text-[var(--text-disabled)]">
                    {provider.name}
                  </div>
                  {providerModels.map((model) => (
                    <button
                      key={model}
                      type="button"
                      onClick={() => {
                        onSelect(provider.id, model);
                        setIsOpen(false);
                      }}
                      className={`flex items-center gap-2 w-full rounded-lg px-2.5 py-1.5 text-left text-sm transition-all ${
                        selectedProviderId === provider.id && selectedModel === model
                          ? 'bg-[var(--primary-soft)] text-[var(--primary)] font-medium'
                          : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
                      }`}
                    >
                      <span className="text-xs font-mono">{model}</span>
                    </button>
                  ))}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
