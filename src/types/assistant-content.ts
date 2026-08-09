/**
 * Assistant content block contract (display layer).
 *
 * Pure types for structured message content rendered by the assistant
 * blocks renderers. This module MUST NOT import from `src/components/**`
 * or any lib that depends on React — keep it a leaf so libs and components
 * can both depend on it without creating an import cycle.
 *
 * Note: this is the renderer-facing shape (its `type` accepts arbitrary
 * strings to stay tolerant of unknown wire blocks). The wire contract lives
 * in `src/lib/assistant-protocol/types.ts` and is the authoritative source
 * for protocol-synced fields; the two remain structurally compatible.
 */

export type BlockType =
  | 'text' | 'reasoning' | 'image' | 'file_reference'
  | 'tool_call' | 'tool_result' | 'diff' | 'citation' | 'error'
  | 'permission' | 'ask_user' | 'plan' | 'subagent' | 'artifact'
  | 'compaction' | 'system_notice' | 'legacy';

export interface ContentBlock {
  type: BlockType | string;
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
  /** True while the parent assistant turn is still streaming this reasoning. */
  live?: boolean;
  locale?: string;
  citationUri?: string;
  citationTitle?: string;
  errorCode?: string;
  errorMessage?: string;
  retryable?: boolean;
  raw?: string;
  originalType?: string;
  diffstat?: string;
  hunks?: Array<{ header: string; lines: string[] }>;
  planMarkdown?: string;
  permissionId?: string;
  artifactId?: string;
  subRunId?: string;
  segmentId?: string;
  summary?: string;
  summaryStatus?: 'completed' | 'failed';
  /** compaction: token counts around a context compression. */
  beforeTokens?: number;
  afterTokens?: number;
  /** system_notice/subagent: structured notice payload (reducer stores data, renderer formats). */
  noticeKind?: string;
  noticeData?: Record<string, unknown>;
}
