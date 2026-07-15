import type { ContentBlock } from '@/components/assistant/blocks';
import type { AssistantFileChange, AssistantRunEvent } from './assistant-types';

export interface AssistantStreamState {
  runId: string;
  status: string;
  lastSequence: number;
  blocks: ContentBlock[];
  fileChanges: AssistantFileChange[];
  usage: { inputTokens: number | null; outputTokens: number | null; reasoningTokens: number | null };
}

export function createAssistantStreamState(runId: string): AssistantStreamState {
  return { runId, status: 'idle', lastSequence: 0, blocks: [], fileChanges: [], usage: { inputTokens: null, outputTokens: null, reasoningTokens: null } };
}

export function reduceAssistantStreamEvent(state: AssistantStreamState, event: AssistantRunEvent): AssistantStreamState {
  if (event.runId !== state.runId || event.sequence <= state.lastSequence) return state;
  const payload = event.payload;
  const next = { ...state, lastSequence: event.sequence, status: state.status === 'idle' ? 'running' : state.status };
  if (event.type === 'completed' || event.type === 'failed' || event.type === 'interrupted') return { ...next, status: event.type };
  if (event.type === 'assistant_delta') {
    const text = String(payload.text ?? payload.delta ?? '');
    const blocks = [...state.blocks];
    const index = blocks.findIndex(block => block.type === 'text');
    if (index >= 0) {
      const existing = blocks[index]!;
      blocks[index] = { ...existing, text: `${existing.text ?? ''}${text}` };
    }
    else blocks.push({ type: 'text', text });
    return { ...next, blocks };
  }
  if (event.type === 'reasoning_delta') {
    const reasoning = String(payload.text ?? payload.reasoning ?? '');
    const blocks = [...state.blocks];
    const index = blocks.findIndex(block => block.type === 'reasoning');
    if (index >= 0) {
      const existing = blocks[index]!;
      blocks[index] = { ...existing, reasoning: `${existing.reasoning ?? ''}${reasoning}` };
    }
    else blocks.unshift({ type: 'reasoning', reasoning });
    return { ...next, blocks };
  }
  if (event.type === 'tool_started') return { ...next, blocks: [...state.blocks, { type: 'tool_call', toolCallId: String(payload.tool_call_id ?? ''), toolName: String(payload.tool_name ?? 'tool'), toolInput: (payload.args ?? {}) as Record<string, unknown>, toolStatus: 'running' }] };
  if (event.type === 'tool_completed' || event.type === 'tool_rejected') {
    const id = String(payload.tool_call_id ?? '');
    const failed = event.type === 'tool_rejected' || payload.status !== 'success';
    return { ...next, blocks: state.blocks.map(block => block.type === 'tool_call' && block.toolCallId === id ? { ...block, toolStatus: failed ? 'failed' : 'completed', toolOutput: payload.output, isError: failed } : block) };
  }
  if (event.type === 'file_changed') return { ...next, fileChanges: [...state.fileChanges, { path: String(payload.path ?? ''), change: String(payload.change_type ?? 'modified') }] };
  if (event.type === 'usage_updated') return { ...next, usage: { inputTokens: numberOrNull(payload.input_tokens), outputTokens: numberOrNull(payload.output_tokens), reasoningTokens: numberOrNull(payload.reasoning_tokens) } };
  return next;
}

function numberOrNull(value: unknown): number | null {
  return typeof value === 'number' ? value : null;
}
