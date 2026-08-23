'use client';

/**
 * Pinned Prompt Snippets Widget (B-034).
 * Quick-copy prompt templates scoped to the current workspace.
 */

import { useState } from 'react';
import { useLocale, t } from '@/i18n';
import { z } from 'zod';
import { Copy, Check } from 'lucide-react';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';

interface PromptSnippetsData {
  snippets: Array<{ id: string; title: string; prompt: string }>;
}

type PromptSnippetsSettings = Record<string, unknown>;

/** 默认提示词模板 id（文案走 i18n：workspace.snippet* 键）。 */
const DEFAULT_SNIPPET_IDS = ['summarize', 'refactor', 'explainError'] as const;

function buildDefaultSnippets(locale: string): PromptSnippetsData['snippets'] {
  const entries: Record<(typeof DEFAULT_SNIPPET_IDS)[number], { titleKey: string; promptKey: string }> = {
    summarize: { titleKey: 'workspace.snippetSummarizeTitle', promptKey: 'workspace.snippetSummarizePrompt' },
    refactor: { titleKey: 'workspace.snippetRefactorTitle', promptKey: 'workspace.snippetRefactorPrompt' },
    explainError: { titleKey: 'workspace.snippetExplainErrorTitle', promptKey: 'workspace.snippetExplainErrorPrompt' },
  };
  return DEFAULT_SNIPPET_IDS.map((id) => ({
    id,
    title: t(locale, entries[id].titleKey),
    prompt: t(locale, entries[id].promptKey),
  }));
}

function PromptSnippetsView(_props: WidgetProps<PromptSnippetsData, PromptSnippetsSettings>) {
  const locale = useLocale();
  const [copiedId, setCopiedId] = useState<string | null>(null);
  // 默认提示词随 locale 渲染（组件级默认值，不写回持久化结构）。
  const snippets = buildDefaultSnippets(locale);

  const handleCopy = (id: string, text: string) => {
    void navigator.clipboard?.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 1500);
  };

  return (
    <div className="flex h-full w-full flex-col justify-between">
      <div className="space-y-1 overflow-y-auto min-h-0 flex-1">
        {snippets.map((snip) => (
          <button
            type="button"
            key={snip.id}
            onClick={() => handleCopy(snip.id, snip.prompt)}
            className="group flex w-full cursor-pointer items-center justify-between rounded-md bg-[var(--surface-hover)] p-1.5 text-xs hover:bg-[var(--primary-soft)] transition-colors text-left"
          >
            <span className="truncate font-medium text-[var(--text)]">{snip.title}</span>
            {copiedId === snip.id ? (
              <Check size={12} className="text-[var(--success)] shrink-0" />
            ) : (
              <Copy size={12} className="text-[var(--text-disabled)] group-hover:text-[var(--primary)] shrink-0" />
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

export const promptSnippetsWidgetDefinition: WidgetDefinition<PromptSnippetsData, PromptSnippetsSettings> = {
  type: 'prompt_snippets',
  titleKey: 'common.prompts',
  descriptionKey: 'common.prompts',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: () => 'workspace.prompts:local',
  // 本地数据源：按当前持久化 locale 提供默认提示词（默认 zh，R-I5）。
  load: async () => {
    const saved = typeof window !== 'undefined' ? await window.nativesAPI?.getLocale?.().catch(() => null) : null;
    return { snippets: buildDefaultSnippets(saved === 'en' ? 'en' : 'zh') };
  },
  Component: PromptSnippetsView,
};

export default promptSnippetsWidgetDefinition;
