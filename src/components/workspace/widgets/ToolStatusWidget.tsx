'use client';

/**
 * AI Tool Status Widget (B-033).
 * 外部 AI 工具（Claude Code / Codex / Gemini CLI 等）接入状态。
 * 当前尚无真实的 CLI 可用性探测数据源；遵循 R-F2 无假数据红线，
 * 不展示写死的可用/不可用勾选，而是诚实地呈现「未接入」空态。
 * 待真实工具探测能力上线后再接 adapter（见 WS-03 备注）。
 */

import { z } from 'zod';
import { useLocale, t } from '@/i18n';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  aiStatusAdapterKey,
  loadAiStatus,
  type AiStatusData,
} from '@/lib/workspace/widgets/adapters/ai-status';

type ToolStatusSettings = Record<string, unknown>;

function ToolStatusView(_props: WidgetProps<AiStatusData, ToolStatusSettings>) {
  const locale = useLocale();
  return (
    <div className="ws-shell-state text-xs text-[var(--text-tertiary)]">
      {t(locale, 'workspace.toolStatusUnavailable')}
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
