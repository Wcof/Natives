'use client';

/**
 * AI Tool Status Widget (B-033).
 * Read-only status for external AI tool integrations (Claude Code, Codex, Gemini CLI, OpenCode).
 */

import { z } from 'zod';
import { CheckCircle2, XCircle } from 'lucide-react';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadAiStatus,
  aiStatusAdapterKey,
  type AiStatusData,
} from '@/lib/workspace/widgets/adapters/ai-status';

type ToolStatusSettings = Record<string, unknown>;

function ToolStatusView(_props: WidgetProps<AiStatusData, ToolStatusSettings>) {
  const tools = [
    { name: 'Claude Code', available: true },
    { name: 'Codex CLI', available: true },
    { name: 'Gemini CLI', available: false },
    { name: 'OpenCode', available: true },
  ];

  return (
    <div className="flex h-full w-full flex-col justify-center">
      <div className="grid grid-cols-2 gap-1.5">
        {tools.map((tool) => (
          <div
            key={tool.name}
            className="flex items-center justify-between rounded-lg bg-[var(--surface-hover)] px-2 py-1 text-xs"
          >
            <span className="truncate text-[var(--text)]">{tool.name}</span>
            {tool.available ? (
              <CheckCircle2 size={12} className="text-[var(--success)] shrink-0" />
            ) : (
              <XCircle size={12} className="text-[var(--text-disabled)] shrink-0" />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

export const toolStatusWidgetDefinition: WidgetDefinition<AiStatusData, ToolStatusSettings> = {
  type: 'tool_status',
  titleKey: 'ai.tools',
  descriptionKey: 'ai.tools',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'small',
  surfacePolicy: { surfaces: ['crystal', 'material', 'plain'], allowBlur: true, allowGlow: false },
  adapterKeyBuilder: aiStatusAdapterKey,
  load: loadAiStatus,
  Component: ToolStatusView,
};

export default toolStatusWidgetDefinition;
