/**
 * Timeline presentation helpers for assistant messages.
 *
 * Live activity (in-progress tools / live reasoning) is shown via a dedicated
 * activity strip driven by run events. Completed tools stay in that strip
 * until the final answer completes, never in the answer body.
 */

import type { ContentBlock } from '@/components/assistant/blocks';
import { computeLineDiff } from '@/lib/diff-utils';
import type { FileChange, RunEvent } from '@/lib/assistant-protocol';

export type TimelineToolStatus = 'pending' | 'running' | 'completed' | 'failed' | 'rejected';

export interface TimelineToolActivity {
  toolCallId: string;
  toolName: string;
  status: TimelineToolStatus;
  parentToolCallId?: string | null;
  depth: number;
  input?: unknown;
  output?: unknown;
  outputText?: string;
  fileChanges?: Array<{ path: string; before: string; after: string }>;
}

export interface ConversationChangeSummary {
  files: Array<{ path: string; additions: number; deletions: number; runId?: string }>;
  additions: number;
  deletions: number;
}

export interface TimelineThinkingActivity {
  text: string;
  live: boolean;
  startedAt?: string | null;
  finishedAt?: string | null;
}

/** Blocks that belong in the durable answer body (Markdown text, plan, etc.). */
export function isBodyContentBlock(block: ContentBlock): boolean {
  const type = block.type;
  if (type === 'tool_call' || type === 'tool_result') return false;
  // Live reasoning is rendered by ThinkingActivity; finished reasoning stays
  // as a collapsible body block when present on the message.
  if (type === 'reasoning' && block.live) return false;
  return true;
}

export function filterTimelineBodyBlocks(blocks: ContentBlock[]): ContentBlock[] {
  return blocks.filter(isBodyContentBlock);
}

export function extractLiveThinking(
  blocks: ContentBlock[],
): TimelineThinkingActivity | null {
  const live = blocks.find((b) => b.type === 'reasoning' && b.live);
  if (!live) return null;
  const text = (live.reasoning ?? live.text ?? '').trim();
  if (!text) {
    return { text: '', live: true };
  }
  return { text, live: true };
}

function inputPaths(value: unknown): string[] {
  const paths: string[] = [];
  const visit = (node: unknown, key = '') => {
    if (typeof node === 'string') {
      if (/^(path|file|file_path|filePath|target|filename)$/i.test(key)) paths.push(node);
      return;
    }
    if (Array.isArray(node)) {
      node.forEach((item) => visit(item));
      return;
    }
    if (node && typeof node === 'object') {
      Object.entries(node as Record<string, unknown>).forEach(([childKey, child]) => visit(child, childKey));
    }
  };
  visit(value);
  return paths;
}

function isToolPathMatch(input: unknown, path: string): boolean {
  return inputPaths(input).some((candidate) => candidate === path || candidate.endsWith(`/${path}`));
}

function isWriteTool(name: string): boolean {
  return /write|patch|edit|replace|delete|remove/.test(name.toLowerCase());
}

/**
 * Derive nested tool activity from a run's event stream.
 * Completed tools remain listed until the run ends so users can see progress;
 * the body filter still hides them from the final answer.
 */
export function deriveToolActivityFromEvents(events: RunEvent[]): TimelineToolActivity[] {
  const byId = new Map<string, TimelineToolActivity>();
  const order: string[] = [];

  for (const event of events) {
    const p = (event.payload ?? {}) as Record<string, unknown>;

    if (event.type === 'file_changed') {
      const path = String(p.path ?? '');
      if (!path) continue;
      const before = typeof p.before === 'string' ? p.before : '';
      const after = typeof p.after === 'string' ? p.after : '';
      const activeTools = [...order]
        .reverse()
        .map((toolId) => byId.get(toolId))
        .filter((candidate): candidate is TimelineToolActivity =>
          candidate !== undefined &&
          (candidate.status === 'pending' || candidate.status === 'running'),
        );
      const tool = activeTools.find((candidate) => isToolPathMatch(candidate.input, path))
        ?? activeTools.find((candidate) => isWriteTool(candidate.toolName));
      if (tool) {
        byId.set(tool.toolCallId, {
          ...tool,
          fileChanges: [...(tool.fileChanges ?? []), { path, before, after }],
        });
      }
      continue;
    }

    if (event.type === 'tool_output_delta') {
      const id = String(p.tool_call_id ?? p.toolCallId ?? '');
      const existing = byId.get(id);
      if (existing) {
        byId.set(id, {
          ...existing,
          outputText: `${existing.outputText ?? ''}${String(p.text ?? '')}`,
        });
      }
      continue;
    }

    const id = String(p.id ?? p.tool_call_id ?? p.toolCallId ?? '');
    if (!id) continue;

    if (
      event.type === 'tool_call_requested' ||
      event.type === 'tool_call_started' ||
      event.type === 'tool_started' ||
      event.type === 'tool_call_delta'
    ) {
      const name = String(p.name ?? p.tool_name ?? p.toolName ?? byId.get(id)?.toolName ?? 'tool');
      const parent = (p.parent_tool_call_id ?? p.parentToolCallId ?? null) as string | null;
      const existing = byId.get(id);
      if (!existing) {
        const depth = parent && byId.has(parent) ? (byId.get(parent)!.depth + 1) : 0;
        byId.set(id, {
          toolCallId: id,
          toolName: name,
          status: event.type === 'tool_call_requested' ? 'pending' : 'running',
          parentToolCallId: parent,
          depth,
          input: p.input,
        });
        order.push(id);
      } else {
        byId.set(id, {
          ...existing,
          toolName: name || existing.toolName,
          input: p.input ?? existing.input,
          status:
            existing.status === 'completed' || existing.status === 'failed'
              ? existing.status
              : event.type === 'tool_call_requested'
                ? 'pending'
                : 'running',
        });
      }
      continue;
    }

    if (
      event.type === 'tool_call_completed' ||
      event.type === 'tool_completed' ||
      event.type === 'tool_rejected'
    ) {
      const name = String(p.name ?? p.tool_name ?? p.toolName ?? byId.get(id)?.toolName ?? 'tool');
      const isError =
        event.type === 'tool_rejected' ||
        Boolean(p.is_error ?? p.isError) ||
        p.status === 'error';
      const existing = byId.get(id);
      if (!existing) {
        byId.set(id, {
          toolCallId: id,
          toolName: name,
          status: isError ? 'failed' : 'completed',
          depth: 0,
          output: p.output,
        });
        order.push(id);
      } else {
        byId.set(id, {
          ...existing,
          toolName: name && name !== 'tool' ? name : existing.toolName,
          status: isError ? 'failed' : 'completed',
          output: p.output ?? existing.output,
        });
      }
    }
  }

  return order.map((id) => byId.get(id)!).filter(Boolean);
}

/** Net file diff for the visible conversation surface (main run may include children). */
export function summarizeConversationChanges(
  events: RunEvent[],
  fileChanges: FileChange[],
): ConversationChangeSummary {
  type ChangeEntry = {
    path: string;
    before?: string;
    after?: string;
    sawBefore: boolean;
    sawAfter: boolean;
    runId?: string;
  };
  const byPath = new Map<string, ChangeEntry>();
  const ensure = (path: string, runId?: string): ChangeEntry => {
    const current = byPath.get(path);
    if (current) return current;
    const next: ChangeEntry = { path, sawBefore: false, sawAfter: false, runId };
    byPath.set(path, next);
    return next;
  };

  for (const event of events) {
    if (event.type !== 'file_changed') continue;
    const payload = (event.payload ?? {}) as Record<string, unknown>;
    const path = String(payload.path ?? '');
    if (!path) continue;
    const entry = ensure(path, event.runId);
    if (!entry.sawBefore && typeof payload.before === 'string') {
      entry.before = payload.before;
      entry.sawBefore = true;
    }
    if (typeof payload.after === 'string') {
      entry.after = payload.after;
      entry.sawAfter = true;
    } else if (Object.prototype.hasOwnProperty.call(payload, 'after')) {
      entry.after = '';
      entry.sawAfter = true;
    }
    entry.runId = event.runId;
  }
  for (const change of fileChanges) ensure(change.path, change.runId);

  const files = [...byPath.values()]
    .map((entry) => {
      const diff = entry.sawBefore && entry.sawAfter
        ? computeLineDiff(entry.before ?? '', entry.after ?? '')
        : null;
      return {
        path: entry.path,
        additions: diff?.additions ?? 0,
        deletions: diff?.deletions ?? 0,
        runId: entry.runId,
      };
    })
    .sort((a, b) => a.path.localeCompare(b.path));
  return {
    files,
    additions: files.reduce((total, file) => total + file.additions, 0),
    deletions: files.reduce((total, file) => total + file.deletions, 0),
  };
}

/** Active (non-terminal) tools for the live strip; completed ones drop out. */
export function selectActiveToolActivity(tools: TimelineToolActivity[]): TimelineToolActivity[] {
  return tools.filter((t) => t.status === 'pending' || t.status === 'running');
}

export function genericThinkingTitle(locale: string): string {
  return locale.startsWith('zh') ? '思考过程' : 'Thinking';
}
