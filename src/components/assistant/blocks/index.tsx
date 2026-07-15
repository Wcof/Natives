// ─── Structured Content Block Renderers ─────────────────
//
// Renderers for all content block types: text, reasoning, image,
// file reference, tool call, tool result, citation, error, and legacy.

import React from 'react';

// ─── Block Types ────────────────────────────────────────

export type BlockType =
  | 'text' | 'reasoning' | 'image' | 'file_reference'
  | 'tool_call' | 'tool_result' | 'citation' | 'error' | 'legacy';

export interface ContentBlock {
  type: BlockType;
  text?: string;
  reasoning?: string;
  signature?: string;
  imageUrl?: string;
  mimeType?: string;
  altText?: string;
  filePath?: string;
  fileSize?: number;
  toolCallId?: string;
  toolName?: string;
  toolInput?: Record<string, unknown>;
  toolStatus?: 'pending' | 'running' | 'completed' | 'failed' | 'rejected';
  toolOutput?: unknown;
  isError?: boolean;
  durationMs?: number;
  citationUri?: string;
  citationTitle?: string;
  errorCode?: string;
  errorMessage?: string;
  retryable?: boolean;
  raw?: string;
  originalType?: string;
}

// ─── Block Renderers ────────────────────────────────────

function TextBlock({ block }: { block: ContentBlock }) {
  return <div className="whitespace-pre-wrap break-words text-[15px] leading-7">{block.text}</div>;
}

function ReasoningBlock({ block }: { block: ContentBlock }) {
  const [expanded, setExpanded] = React.useState(false);
  return (
    <div className="my-2 rounded-lg bg-[var(--surface-hover)] px-3 py-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="text-xs text-[var(--text-disabled)] hover:text-[var(--text-secondary)] transition-colors"
        aria-expanded={expanded}
      >
        {expanded ? '隐藏思考过程' : '查看思考过程'}
      </button>
      {expanded && block.reasoning && (
        <div className="mt-1 text-sm text-[var(--text-secondary)] italic">
          {block.reasoning}
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

function ToolCallBlock({ block }: { block: ContentBlock }) {
  const [expanded, setExpanded] = React.useState(block.toolStatus === 'failed');
  const statusColor = {
    pending: 'var(--warning)',
    running: 'var(--primary)',
    completed: 'var(--success)',
    failed: 'var(--danger)',
    rejected: 'var(--text-disabled)',
  }[block.toolStatus || 'pending'];

  return (
    <div className="my-2 overflow-hidden rounded-lg border border-[var(--border-subtle)]">
      <button type="button" onClick={() => setExpanded(value => !value)} className="flex w-full items-center gap-2 bg-[var(--surface-hover)] px-3 py-2 text-left text-xs font-medium">
        <span className="w-2 h-2 rounded-full" style={{ backgroundColor: statusColor }} />
        <span className="font-mono">{block.toolName}</span>
        {block.toolStatus && (
          <span className="text-[var(--text-disabled)] ml-auto">{block.toolStatus}</span>
        )}
      </button>
      {expanded && block.toolInput && (
        <div className="px-3 py-2 text-xs font-mono text-[var(--text-secondary)] overflow-x-auto">
          <pre className="whitespace-pre-wrap">{JSON.stringify(block.toolInput, null, 2)}</pre>
        </div>
      )}
      {block.toolOutput !== undefined && (
        <div className={`px-3 py-2 text-xs font-mono border-t ${block.isError ? 'border-red-400/30 bg-red-50 dark:bg-red-950/20' : ''}`}>
          <pre className="whitespace-pre-wrap">{typeof block.toolOutput === 'string' ? block.toolOutput : JSON.stringify(block.toolOutput, null, 2)}</pre>
        </div>
      )}
    </div>
  );
}

function ToolResultBlock({ block }: { block: ContentBlock }) {
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
    case 'citation':
      return <CitationBlock key={index} block={block} />;
    case 'error':
      return <ErrorBlock key={index} block={block} />;
    case 'legacy':
      return <LegacyBlock key={index} block={block} />;
    default:
      return <div key={index} className="text-xs text-[var(--text-disabled)]">Unknown block type</div>;
  }
}

// ─── Block List Renderer ────────────────────────────────

export function renderBlocks(blocks: ContentBlock[]): React.ReactNode[] {
  return blocks.map((block, index) => renderBlock(block, index));
}
