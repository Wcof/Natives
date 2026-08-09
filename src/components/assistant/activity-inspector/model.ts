'use client';

import { Play, ListTree, FileDiff, Package, Brain, Radio, type LucideIcon } from 'lucide-react';
import type { BackgroundTask, RunEvent } from '@/lib/assistant-protocol';
import type { InspectorTab } from '@/lib/assistant-workspace';
import type { TodoStatus } from '@/lib/assistant-activity-view';

export interface ActivitySubagentView {
  id: string;
  name: string;
  status: string;
  providerId?: string;
  keyLabel?: string;
  childConversationId?: string;
  task?: string;
  todos?: Array<{ id: string; content: string; status: TodoStatus }>;
  /** Failure message — never a raw key. */
  error?: string;
}

export const TABS: Array<{ id: InspectorTab; labelKey: string; icon: LucideIcon; devOnly?: boolean }> = [
  { id: 'run', labelKey: 'activityInspector.tabRun', icon: Play },
  { id: 'tasks', labelKey: 'activityInspector.tabTasks', icon: ListTree },
  { id: 'changes', labelKey: 'activityInspector.tabChanges', icon: FileDiff },
  { id: 'artifacts', labelKey: 'activityInspector.tabArtifacts', icon: Package },
  { id: 'context', labelKey: 'activityInspector.tabContext', icon: Brain },
  { id: 'events', labelKey: 'activityInspector.tabEvents', icon: Radio, devOnly: true },
];

export function mapWireBackgroundTask(raw: Record<string, unknown>): BackgroundTask {
  const id = String(raw.id ?? '');
  const kindRaw = String(raw.kind ?? 'other');
  const kind: BackgroundTask['kind'] =
    kindRaw === 'subagent' ||
    kindRaw === 'terminal' ||
    kindRaw === 'monitor' ||
    kindRaw === 'scheduler'
      ? kindRaw
      : 'other';
  const output =
    typeof raw.output === 'string'
      ? raw.output
      : raw.output == null
        ? null
        : String(raw.output);
  const title =
    typeof raw.title === 'string' && raw.title.trim()
      ? raw.title
      : output
        ? output.slice(0, 80)
        : id.slice(0, 8) || 'task';
  return {
    id,
    kind,
    runId: raw.run_id != null ? String(raw.run_id) : raw.runId != null ? String(raw.runId) : undefined,
    conversationId:
      raw.conversation_id != null
        ? String(raw.conversation_id)
        : raw.conversationId != null
          ? String(raw.conversationId)
          : undefined,
    title,
    status: String(raw.status ?? 'unknown'),
    createdAt: String(raw.created_at ?? raw.createdAt ?? ''),
    error: raw.error != null ? String(raw.error) : undefined,
    output,
  };
}

export function snippet(text: string | null | undefined, max = 120): string {
  if (!text) return '';
  const oneLine = text.replace(/\s+/g, ' ').trim();
  if (oneLine.length <= max) return oneLine;
  return `${oneLine.slice(0, max)}…`;
}

export interface DeltaEventGroup {
  type: string;
  first: number;
  last: number;
  count: number;
}

/**
 * Delta events are stream chunks, not repeated tool calls. Keep lifecycle
 * events individual while compacting adjacent delta chunks for inspection.
 */
export function groupDeltaEvents(events: RunEvent[]): DeltaEventGroup[] {
  const groups: DeltaEventGroup[] = [];
  for (const event of events) {
    const previous = groups.at(-1);
    const compactable = event.type === 'reasoning_delta' || event.type === 'text_delta';
    if (compactable && previous?.type === event.type && previous.last + 1 === event.sequence) {
      previous.last = event.sequence;
      previous.count += 1;
    } else {
      groups.push({ type: event.type, first: event.sequence, last: event.sequence, count: 1 });
    }
  }
  return groups;
}
