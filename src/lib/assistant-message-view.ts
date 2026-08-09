import { t } from '@/i18n';
import type { ContentBlock } from '@/types/assistant-content';

export function formatElapsed(durationMs: number | null | undefined): string | null {
  if (durationMs == null || !Number.isFinite(durationMs) || durationMs < 100) {
    return null;
  }
  const totalSec = durationMs / 1000;
  if (totalSec < 60) {
    return `${totalSec.toFixed(1)}s`;
  }
  const totalMin = Math.floor(totalSec / 60);
  const remSecVal = totalSec % 60;
  const remSecStr = remSecVal.toFixed(1).padStart(4, '0'); // e.g. "00.0", "05.2", "11.1"
  if (totalMin < 60) {
    return `${totalMin}m${remSecStr}s`;
  }
  const hours = Math.floor(totalMin / 60);
  const remMinStr = Math.floor(totalMin % 60).toString().padStart(2, '0');
  return `${hours}h${remMinStr}m${remSecStr}s`;
}

/**
 * Format total run time with status prefix:
 * e.g., "运行中 11.1s" / "已完成 1h34m11.1s"
 */
export function formatRunElapsed(
  durationMs: number | null | undefined,
  isFinished = true,
  locale = 'zh',
): string | null {
  const formatted = formatElapsed(durationMs);
  if (!formatted) return null;
  if (isFinished) {
    return t(locale, 'messageView.completedElapsed', { formatted });
  }
  return t(locale, 'messageView.runningElapsed', { formatted });
}

/** Localized duration for reasoning headers, e.g. "3.2 秒" / "3.2s". Returns null if duration is under 0.1s. */
export function formatReasoningDuration(milliseconds: number, locale = 'zh'): string | null {
  const seconds = Math.max(0, milliseconds) / 1000;
  if (seconds < 0.1) return null;
  if (seconds < 60) return t(locale, 'messageView.reasoningSeconds', { v: (Math.round(seconds * 10) / 10).toFixed(1) });
  return t(locale, 'messageView.reasoningMinutesSeconds', {
    m: Math.floor(seconds / 60),
    s: Math.floor(seconds % 60),
  });
}

const LIVE_PHASE_KEYS = [
  'messageView.phaseThinking',
  'messageView.phaseAnalyzing',
  'messageView.phasePlanning',
  'messageView.phaseOrganizing',
] as const;

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
  const seconds = Math.max(0, durationMs ?? 0) / 1000;
  const index = Math.min(LIVE_PHASE_KEYS.length - 1, Math.floor(seconds / 4));
  return t(locale, LIVE_PHASE_KEYS[index]!);
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
  const duration = options.durationMs != null && Number.isFinite(options.durationMs)
    ? formatReasoningDuration(options.durationMs, locale)
    : null;

  if (options.live) {
    const stage = extractReasoningStage(options.reasoning ?? '')
      ?? livePhaseFallback(options.durationMs, locale);
    return duration ? `${stage} · ${duration}` : stage;
  }

  if (options.expanded) {
    return t(locale, 'messageView.hideThinking');
  }
  if (duration) {
    return t(locale, 'messageView.thoughtFor', { duration });
  }
  return t(locale, 'messageView.viewThinking');
}

export function messagePlainText(blocks: ContentBlock[]): string {
  return blocks.filter(block => block.type === 'text').map(block => block.text ?? '').filter(Boolean).join('\n');
}
