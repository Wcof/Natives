import type { ContentBlock } from '@/components/assistant/blocks';

export function formatElapsed(milliseconds: number): string {
  const seconds = Math.max(0, milliseconds) / 1000;
  if (seconds < 60) return `${(Math.round(seconds * 10) / 10).toFixed(1)}s`;
  return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
}

/** Localized duration for reasoning headers, e.g. "3.2 秒" / "3.2s". */
export function formatReasoningDuration(milliseconds: number, locale = 'zh'): string {
  const seconds = Math.max(0, milliseconds) / 1000;
  if (locale.startsWith('zh')) {
    if (seconds < 60) return `${(Math.round(seconds * 10) / 10).toFixed(1)} 秒`;
    return `${Math.floor(seconds / 60)} 分 ${Math.floor(seconds % 60)} 秒`;
  }
  return formatElapsed(milliseconds);
}

const LIVE_PHASES_ZH = ['正在思考', '分析问题', '梳理思路', '组织回答'] as const;
const LIVE_PHASES_EN = ['Thinking', 'Analyzing', 'Planning', 'Organizing'] as const;

function cleanStageLabel(value: string): string {
  const cleaned = value.replace(/\s+/g, ' ').trim();
  if (cleaned.length <= 40) return cleaned;
  return `${cleaned.slice(0, 37)}…`;
}

/**
 * Pull a human-readable "current stage" title from free-form reasoning text.
 * Prefers headings / step markers, then a short recent line.
 */
export function extractReasoningStage(reasoning: string): string | null {
  const text = reasoning.trim();
  if (!text) return null;

  const headings = [...text.matchAll(/^(#{1,6})\s+(.+)$/gm)];
  if (headings.length > 0) {
    return cleanStageLabel(headings[headings.length - 1]![2]!);
  }

  const steps = [...text.matchAll(
    /^(?:Step\s*\d+[:.\s、]+|第[0-9一二三四五六七八九十百]+步[:.\s、]*|\d+[.、]\s+)(.+)$/gim,
  )];
  if (steps.length > 0) {
    return cleanStageLabel(steps[steps.length - 1]![1]!);
  }

  const bolds = [...text.matchAll(/^\*\*(.+?)\*\*\s*$/gm)];
  if (bolds.length > 0) {
    return cleanStageLabel(bolds[bolds.length - 1]![1]!);
  }

  const lines = text.split(/\n+/).map(line => line.trim().replace(/^[-*•]\s+/, '')).filter(Boolean);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index]!;
    // Prefer short title-like lines over full prose sentences.
    if (line.length >= 2 && line.length <= 36 && !/[.。!！?？;；:：]$/.test(line)) {
      return cleanStageLabel(line);
    }
  }

  const first = lines[0];
  if (!first) return null;
  return cleanStageLabel(first);
}

function livePhaseFallback(durationMs: number | undefined, locale: string): string {
  const phases = locale.startsWith('zh') ? LIVE_PHASES_ZH : LIVE_PHASES_EN;
  const seconds = Math.max(0, durationMs ?? 0) / 1000;
  const index = Math.min(phases.length - 1, Math.floor(seconds / 4));
  return phases[index]!;
}

/**
 * Toggle / header label for a reasoning block.
 * Live: dynamic stage name + elapsed time.
 * Done: "思考了 N 秒" when collapsed; hide label when expanded.
 */
export function reasoningToggleLabel(options: {
  reasoning?: string;
  live?: boolean;
  expanded?: boolean;
  durationMs?: number;
  locale?: string;
}): string {
  const locale = options.locale ?? 'zh';
  const zh = locale.startsWith('zh');
  const duration = options.durationMs != null && Number.isFinite(options.durationMs)
    ? formatReasoningDuration(options.durationMs, locale)
    : null;

  if (options.live) {
    const stage = extractReasoningStage(options.reasoning ?? '')
      ?? livePhaseFallback(options.durationMs, locale);
    return duration ? `${stage} · ${duration}` : stage;
  }

  if (options.expanded) {
    return zh ? '隐藏思考过程' : 'Hide thinking';
  }
  if (duration) {
    return zh ? `思考了 ${duration}` : `Thought for ${duration}`;
  }
  return zh ? '查看思考过程' : 'View thinking';
}

export function messagePlainText(blocks: ContentBlock[]): string {
  return blocks.filter(block => block.type === 'text').map(block => block.text ?? '').filter(Boolean).join('\n');
}
