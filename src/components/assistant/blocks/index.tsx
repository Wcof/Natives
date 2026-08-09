// ─── Structured Content Block Renderers ─────────────────
//
// Renderers for all content block types: text, reasoning, image,
// file reference, tool call, tool result, citation, error, and legacy.

import React from 'react';
import { reasoningToggleLabel } from '@/lib/assistant-message-view';
import { t, useLocale } from '@/i18n';
import type { ContentBlock } from '@/types/assistant-content';
import MarkdownText from '../MarkdownText';

// ─── Block Renderers ────────────────────────────────────

function TextBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="min-w-0 max-w-full break-words text-[15px] leading-7 text-[var(--text)]">
      <MarkdownText source={block.text} />
    </div>
  );
}

function ReasoningBlock({ block }: { block: ContentBlock }) {
  const live = Boolean(block.live);
  const zh = (block.locale ?? 'zh').startsWith('zh');
  const [expanded, setExpanded] = React.useState(live);
  const userOverrideRef = React.useRef(false);

  React.useEffect(() => {
    if (userOverrideRef.current) return;
    setExpanded(live);
  }, [live]);

  const label = reasoningToggleLabel({
    reasoning: block.reasoning,
    live,
    expanded,
    durationMs: block.durationMs,
    locale: block.locale,
  });

  const hasSummary = Boolean(!live && block.summary);
  const summaryFailed = Boolean(!live && block.summaryStatus === 'failed');

  return (
    <div className="my-3 border-b border-[var(--border-subtle)] py-2">
      <button
        type="button"
        onClick={() => {
          userOverrideRef.current = true;
          setExpanded(value => !value);
        }}
        className={`inline-flex items-center gap-1.5 text-xs transition-colors ${
          live
            ? 'text-[var(--text-secondary)]'
            : 'text-[var(--text-disabled)] hover:text-[var(--text-secondary)]'
        }`}
        aria-expanded={expanded}
      >
        {live && (
          <span className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-[var(--primary)]" aria-hidden />
        )}
        <span>{label}</span>
        {summaryFailed && (
          <span className="text-[10px] text-[var(--warning)] ml-1">
            ({zh ? '总结生成失败' : 'Summary failed'})
          </span>
        )}
      </button>

      {/* Completed state summary (when collapsed and summary exists) */}
      {!expanded && hasSummary && (
        <div className="mt-1 text-xs text-[var(--text-secondary)] italic">
          {block.summary}
        </div>
      )}

      {/* Full reasoning body when expanded — Markdown (headings/lists/code in thinking) */}
      {expanded && block.reasoning && (
        <div className="mt-1 max-h-[280px] overflow-y-auto rounded bg-[var(--surface-hover)]/60 px-2 py-1 text-sm text-[var(--text-secondary)] italic">
          <MarkdownText source={block.reasoning} />
        </div>
      )}
    </div>
  );
}

function ImageBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="my-2">
      {block.imageUrl ? (
        <img
          src={block.imageUrl}
          alt={block.altText || 'Image'}
          className="max-w-full rounded-lg max-h-96 object-contain"
          loading="lazy"
        />
      ) : (
        <div className="text-sm text-[var(--text-disabled)]">[Image]</div>
      )}
    </div>
  );
}

function FileReferenceBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--surface-hover)] my-1">
      <span className="text-xs font-mono text-[var(--text-secondary)] truncate max-w-[200px]">
        {block.filePath}
      </span>
      {block.fileSize && (
        <span className="text-xs text-[var(--text-disabled)]">
          {(block.fileSize / 1024).toFixed(1)} KB
        </span>
      )}
    </div>
  );
}

function defaultToolExpanded(status: ContentBlock['toolStatus']): boolean {
  // Running/pending tools expand; completed collapse; failed stay open.
  if (status === 'failed') return true;
  if (status === 'running' || status === 'pending') return true;
  return false;
}

function ToolCallBlock({ block }: { block: ContentBlock }) {
  const status = block.toolStatus || 'pending';
  const [expanded, setExpanded] = React.useState(() => defaultToolExpanded(status));
  const userOverrideRef = React.useRef(false);
  const prevStatusRef = React.useRef(status);

  React.useEffect(() => {
    if (userOverrideRef.current) return;
    // Only auto-adjust when status actually transitions (e.g. running → completed).
    if (prevStatusRef.current !== status) {
      setExpanded(defaultToolExpanded(status));
      prevStatusRef.current = status;
    }
  }, [status]);

  const statusColor = {
    pending: 'var(--warning)',
    running: 'var(--primary)',
    completed: 'var(--success)',
    failed: 'var(--danger)',
    rejected: 'var(--text-disabled)',
  }[status];

  const durationLabel =
    typeof block.durationMs === 'number' && block.durationMs >= 0
      ? block.durationMs >= 1000
        ? `${(block.durationMs / 1000).toFixed(1)}s`
        : `${block.durationMs}ms`
      : null;

  const isTerminal = block.toolName === 'run_terminal';
  const terminalOutput =
    isTerminal && block.toolOutput && typeof block.toolOutput === 'object'
      ? (block.toolOutput as Record<string, unknown>)
      : null;
  const displayCommand =
    (terminalOutput?.display_command as string | undefined) ||
    (block.toolInput && typeof block.toolInput === 'object'
      ? String((block.toolInput as Record<string, unknown>).command ?? '')
      : '');
  const exitCode =
    terminalOutput && 'exit_code' in terminalOutput
      ? (terminalOutput.exit_code as number | null)
      : null;
  const truncated = Boolean(terminalOutput?.truncated);
  const bg = Boolean(terminalOutput?.background || terminalOutput?.auto_backgrounded);

  return (
    <div
      className="my-2 overflow-hidden rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)]/40"
      data-tool-name={block.toolName}
      data-tool-status={status}
    >
      <button
        type="button"
        onClick={() => {
          userOverrideRef.current = true;
          setExpanded((value) => !value);
        }}
        className="flex w-full items-center gap-2 bg-[var(--surface-hover)] px-3 py-2 text-left text-xs font-medium text-[var(--text-secondary)]"
      >
        <span className="w-2 h-2 rounded-full" style={{ backgroundColor: statusColor }} />
        <span className="font-mono truncate">
          {isTerminal && displayCommand ? displayCommand : block.toolName}
        </span>
        <span className="ml-auto flex items-center gap-2 text-[var(--text-disabled)] shrink-0">
          {isTerminal && exitCode !== null && exitCode !== undefined && (
            <span className="tabular-nums">exit {String(exitCode)}</span>
          )}
          {bg && <span>bg</span>}
          {truncated && <span>trunc</span>}
          {durationLabel && <span className="tabular-nums">{durationLabel}</span>}
          <span>{status}</span>
        </span>
      </button>
      {expanded && (
        <div className="max-h-[280px] overflow-auto border-t border-[var(--border-subtle)]">
          {block.toolInput !== undefined && !isTerminal && (
            <div className="px-3 py-2 text-xs font-mono text-[var(--text-secondary)]">
              <div className="mb-1 text-[10px] uppercase tracking-wide text-[var(--text-disabled)]">input</div>
              <pre className="whitespace-pre-wrap">{JSON.stringify(block.toolInput, null, 2)}</pre>
            </div>
          )}
          {block.toolOutput !== undefined && (
            <div
              className={`px-3 py-2 text-xs font-mono border-t border-[var(--border-subtle)] ${
                block.isError ? 'border-red-400/30 bg-red-50 dark:bg-red-950/20 text-[var(--danger)]' : 'text-[var(--text-secondary)]'
              }`}
            >
              <div className="mb-1 text-[10px] uppercase tracking-wide text-[var(--text-disabled)]">
                {isTerminal ? 'terminal output' : 'output'}
              </div>
              <pre className="whitespace-pre-wrap">
                {isTerminal && terminalOutput
                  ? String(terminalOutput.output ?? terminalOutput.stdout ?? JSON.stringify(terminalOutput, null, 2))
                  : typeof block.toolOutput === 'string'
                    ? block.toolOutput
                    : JSON.stringify(block.toolOutput, null, 2)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ToolResultBlock({ block }: { block: ContentBlock }) {
  // Batch 3: a structured card for the create_creative_draft handoff instead of
  // a raw JSON dump — the draft belongs to the Personal Creations surface.
  if (
    block.toolName === 'create_creative_draft' &&
    (block.toolOutput as Record<string, unknown> | undefined)?.draftId
  ) {
    return <CreativeDraftCreatedCard block={block} />;
  }
  return (
    <div className={`border rounded-lg my-2 overflow-hidden ${block.isError ? 'border-red-400/30' : ''}`}>
      <div className="flex items-center gap-2 px-3 py-2 text-xs">
        <span className={`w-2 h-2 rounded-full ${block.isError ? 'bg-red-500' : 'bg-green-500'}`} />
        <span className="font-mono text-[var(--text-secondary)]">
          Tool Result{block.durationMs ? ` (${block.durationMs}ms)` : ''}
        </span>
      </div>
      {block.toolOutput !== undefined && (
        <div className="px-3 py-2 text-xs font-mono border-t overflow-x-auto">
          <pre className="whitespace-pre-wrap">{JSON.stringify(block.toolOutput, null, 2)}</pre>
        </div>
      )}
    </div>
  );
}

/** Structured result card for `create_creative_draft` (batch 3). */
function CreativeDraftCreatedCard({ block }: { block: ContentBlock }) {
  const locale = useLocale();
  const out = (block.toolOutput ?? {}) as Record<string, unknown>;
  const draftId = String(out.draftId ?? '');
  const name = String(out.name ?? '');
  const previewUrl = String(out.previewUrl ?? '');
  return (
    <div className="border rounded-lg my-2 overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 text-xs">
        <span className="w-2 h-2 rounded-full bg-emerald-500" />
        <span className="font-medium">{t(locale, 'creative.draftCreatedTitle')}</span>
      </div>
      <div className="px-3 py-2 text-xs border-t space-y-1">
        <div>
          {t(locale, 'creative.draftCreatedName')}: {name || '—'}
        </div>
        <div className="font-mono text-[var(--text-secondary)]">
          {t(locale, 'creative.draftCreatedId')}: {draftId}
        </div>
        {previewUrl && (
          <div className="font-mono text-[var(--text-secondary)]">/drafts/{draftId}/</div>
        )}
        <div className="text-[var(--text-secondary)]">{t(locale, 'creative.draftCreatedHint')}</div>
      </div>
    </div>
  );
}

function CitationBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="flex items-start gap-2 px-3 py-2 my-1 text-xs text-[var(--text-secondary)] border-l-2 border-[var(--border)]">
      <span className="text-[var(--text-disabled)]">📚</span>
      <div>
        <div className="font-medium">{block.citationTitle || 'Source'}</div>
        {block.citationUri && (
          <a href={block.citationUri} target="_blank" rel="noopener noreferrer" className="underline hover:text-[var(--primary)]">
            {block.citationUri}
          </a>
        )}
      </div>
    </div>
  );
}

function ErrorBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="border border-red-400/30 rounded-lg px-3 py-2 my-2 bg-red-50 dark:bg-red-950/20">
      <div className="flex items-center gap-2 text-xs font-medium text-red-600 dark:text-red-400">
        <span>Error{block.errorCode ? ` (${block.errorCode})` : ''}</span>
        {block.retryable && <span className="text-[var(--text-disabled)]">— Retryable</span>}
      </div>
      {block.errorMessage && (
        <div className="mt-1 text-xs text-red-500 dark:text-red-400">{block.errorMessage}</div>
      )}
    </div>
  );
}

function LegacyBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="border border-dashed border-[var(--border)] rounded-lg px-3 py-2 my-2">
      <div className="text-xs text-[var(--text-disabled)] mb-1">
        Legacy block ({block.originalType || 'unknown'})
      </div>
      <div className="text-sm text-[var(--text-secondary)] whitespace-pre-wrap">{block.raw}</div>
    </div>
  );
}

// ─── Main Block Renderer ────────────────────────────────

function DiffBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="my-2 overflow-hidden rounded-lg border border-[var(--border-subtle)]">
      <div className="bg-[var(--surface-hover)] px-3 py-1.5 text-xs font-medium">
        Diff{block.diffstat ? ` · ${block.diffstat}` : ''}{block.filePath ? ` · ${block.filePath}` : ''}
      </div>
      <pre className="max-h-64 overflow-auto px-3 py-2 text-xs font-mono">
        {(block.hunks ?? []).map((h, i) => (
          <div key={i}>
            <div className="text-[var(--text-disabled)]">{h.header}</div>
            {h.lines.map((line, j) => (
              <div
                key={j}
                className={
                  line.startsWith('+')
                    ? 'bg-green-500/10 text-green-700 dark:text-green-300'
                    : line.startsWith('-')
                      ? 'bg-red-500/10 text-red-700 dark:text-red-300'
                      : ''
                }
              >
                {line}
              </div>
            ))}
          </div>
        ))}
        {!block.hunks?.length && block.raw && <div className="whitespace-pre-wrap">{block.raw}</div>}
      </pre>
    </div>
  );
}

function PlanBlock({ block }: { block: ContentBlock }) {
  return (
    <div className="my-2 min-w-0 max-w-full rounded-lg border border-[var(--border)] p-3 text-sm">
      <div className="mb-1 text-xs font-medium text-[var(--text-secondary)]">Plan</div>
      <MarkdownText source={block.planMarkdown ?? block.text} />
    </div>
  );
}

function formatTokenCount(value: number | undefined): string {
  const n = Number(value ?? 0);
  if (!Number.isFinite(n) || n <= 0) return '0';
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(Math.round(n));
}

/** Context compression divider — collapsed bar with expandable summary. */
function CompactionBlock({ block }: { block: ContentBlock }) {
  const appLocale = useLocale();
  const zh = (block.locale ?? appLocale).startsWith('zh');
  const [expanded, setExpanded] = React.useState(false);
  const before = formatTokenCount(block.beforeTokens);
  const after = formatTokenCount(block.afterTokens);
  // i18n-pending: inline zh/en until assistant block strings converge in src/i18n
  const label = zh
    ? `上下文已压缩 ${before} → ${after} tokens`
    : `Context compressed ${before} → ${after} tokens`;
  const hasSummary = Boolean(block.summary && block.summary.trim());

  return (
    <div className="my-3" data-block-type="compaction">
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        disabled={!hasSummary}
        aria-expanded={expanded}
        className="flex w-full items-center gap-2 text-[11px] text-[var(--text-disabled)] hover:text-[var(--text-secondary)] disabled:cursor-default"
      >
        <span className="h-px flex-1 bg-[var(--border-subtle)]" aria-hidden />
        <span className="shrink-0">{label}</span>
        {hasSummary && (
          <span className="shrink-0" aria-hidden>
            {expanded ? '▾' : '▸'}
          </span>
        )}
        <span className="h-px flex-1 bg-[var(--border-subtle)]" aria-hidden />
      </button>
      {expanded && hasSummary && (
        <div className="mt-2 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-hover)]/50 px-3 py-2 text-xs text-[var(--text-secondary)]">
          <div className="mb-1 text-[10px] uppercase tracking-wide text-[var(--text-disabled)]">
            {zh ? '压缩摘要' : 'Compaction summary'}
          </div>
          <MarkdownText source={block.summary} />
        </div>
      )}
    </div>
  );
}

/**
 * Timeline event strip for system notices (retry / checkpoint / subagent …).
 * Structured data comes from `noticeKind` + `noticeData`; plain notices fall
 * back to `text` / `raw`. Unknown kinds must render without crashing.
 */
function SystemNoticeBlock({ block }: { block: ContentBlock }) {
  const appLocale = useLocale();
  const zh = (block.locale ?? appLocale).startsWith('zh');
  const [showDetail, setShowDetail] = React.useState(false);
  const data = (block.noticeData ?? {}) as Record<string, unknown>;

  // i18n-pending: inline zh/en until assistant block strings converge in src/i18n
  let tone: 'info' | 'warning' = 'info';
  let label: string;
  let detail: string | null = null;

  switch (block.noticeKind) {
    case 'generation_retry': {
      tone = 'warning';
      const attempt = Number(data.attempt ?? 0);
      const max = Number(data.maxAttempts ?? 0);
      const code = String(data.code ?? '');
      const nOfM = max > 0 ? `${attempt}/${max}` : `${attempt}`;
      label = zh
        ? `上游响应失败，第 ${nOfM} 次重试${code ? `（${code}）` : ''}`
        : `Upstream attempt failed, retry ${nOfM}${code ? ` (${code})` : ''}`;
      break;
    }
    case 'checkpoint_created': {
      const cpLabel = data.label != null ? String(data.label) : '';
      label = zh ? '已创建还原点' : 'Restore point created';
      detail = cpLabel || null;
      break;
    }
    case 'checkpoint_rewound': {
      const count = Number(data.count ?? (Array.isArray(data.paths) ? data.paths.length : 0));
      label = zh ? `已回滚 ${count} 个文件` : `Rolled back ${count} file${count === 1 ? '' : 's'}`;
      detail = Array.isArray(data.paths) && data.paths.length > 0
        ? data.paths.map(String).join('\n')
        : null;
      break;
    }
    case 'subagent_created': {
      const task = String(data.task ?? '');
      label = zh ? '派生子任务' : 'Subtask spawned';
      detail = task || null;
      break;
    }
    default: {
      const fallback = block.text ?? block.raw ?? '';
      label = fallback || `[${String(block.originalType ?? block.type)}]`;
      break;
    }
  }

  return (
    <div
      className="my-2 flex items-start gap-2 rounded-md bg-[var(--surface-hover)]/70 px-3 py-1.5 text-xs text-[var(--text-secondary)]"
      data-block-type={block.type}
      data-notice-kind={block.noticeKind}
    >
      <span
        className="mt-1 h-1.5 w-1.5 shrink-0 rounded-full"
        style={{ backgroundColor: tone === 'warning' ? 'var(--warning)' : 'var(--text-disabled)' }}
        aria-hidden
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="min-w-0 break-words">{label}</span>
          {detail && (
            <button
              type="button"
              onClick={() => setShowDetail((value) => !value)}
              aria-expanded={showDetail}
              className="shrink-0 text-[10px] text-[var(--text-disabled)] underline hover:text-[var(--text-secondary)]"
            >
              {showDetail ? (zh ? '收起' : 'Hide') : (zh ? '详情' : 'Details')}
            </button>
          )}
        </div>
        {showDetail && detail && (
          <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] text-[var(--text-disabled)]">
            {detail}
          </pre>
        )}
      </div>
    </div>
  );
}

export function renderBlock(block: ContentBlock, index: number): React.ReactNode {
  switch (block.type) {
    case 'text':
      return <TextBlock key={index} block={block} />;
    case 'reasoning':
      return <ReasoningBlock key={index} block={block} />;
    case 'image':
      return <ImageBlock key={index} block={block} />;
    case 'file_reference':
      return <FileReferenceBlock key={index} block={block} />;
    case 'tool_call':
      return <ToolCallBlock key={index} block={block} />;
    case 'tool_result':
      return <ToolResultBlock key={index} block={block} />;
    case 'diff':
      return <DiffBlock key={index} block={block} />;
    case 'citation':
      return <CitationBlock key={index} block={block} />;
    case 'error':
      return <ErrorBlock key={index} block={block} />;
    case 'plan':
      return <PlanBlock key={index} block={block} />;
    case 'compaction':
      return <CompactionBlock key={index} block={block} />;
    case 'system_notice':
    case 'subagent':
      return <SystemNoticeBlock key={index} block={block} />;
    case 'artifact':
    case 'permission':
    case 'ask_user':
      return <SystemNoticeBlock key={index} block={{ ...block, text: block.text ?? `[${block.type}]` }} />;
    case 'legacy':
      return <LegacyBlock key={index} block={block} />;
    default:
      // Unknown types must not crash the timeline
      return (
        <LegacyBlock
          key={index}
          block={{
            type: 'legacy',
            originalType: String(block.type),
            raw: block.raw ?? JSON.stringify(block, null, 2),
          }}
        />
      );
  }
}

// ─── Block List Renderer ────────────────────────────────

export function renderBlocks(blocks: ContentBlock[]): React.ReactNode[] {
  return blocks.map((block, index) => renderBlock(block, index));
}
