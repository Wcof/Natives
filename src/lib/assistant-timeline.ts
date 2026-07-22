/**
 * Timeline presentation helpers for assistant messages.
 *
 * Live activity (in-progress tools / live reasoning) is shown via a dedicated
 * activity strip driven by run events. Completed tools stay out of the main
 * answer body; the activity panel keeps full history via eventsByRun.
 */

import type { ContentBlock } from '@/components/assistant/blocks';
import type { RunEvent } from '@/lib/assistant-protocol';

export type TimelineToolStatus = 'pending' | 'running' | 'completed' | 'failed' | 'rejected';

export interface TimelineToolActivity {
  toolCallId: string;
  toolName: string;
  status: TimelineToolStatus;
  parentToolCallId?: string | null;
  depth: number;
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

function mapToolStatus(raw: string | undefined): TimelineToolStatus {
  switch (raw) {
    case 'completed':
    case 'failed':
    case 'rejected':
    case 'pending':
    case 'running':
      return raw;
    case 'error':
      return 'failed';
    default:
      return 'running';
  }
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
        });
        order.push(id);
      } else {
        byId.set(id, {
          ...existing,
          toolName: name || existing.toolName,
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
        });
        order.push(id);
      } else {
        byId.set(id, {
          ...existing,
          toolName: name || existing.toolName,
          status: isError ? 'failed' : 'completed',
        });
      }
    }
  }

  return order.map((id) => byId.get(id)!).filter(Boolean);
}

/** Active (non-terminal) tools for the live strip; completed ones drop out. */
export function selectActiveToolActivity(tools: TimelineToolActivity[]): TimelineToolActivity[] {
  return tools.filter((t) => t.status === 'pending' || t.status === 'running');
}

export function genericThinkingTitle(locale: string): string {
  return locale.startsWith('zh') ? '思考过程' : 'Thinking';
}
