/**
 * Golden fixtures for GUI Phase 0+ — drive FixtureAdapter + reducer tests.
 */
import type { Conversation, Message, Run, RunEvent } from '@/lib/assistant-protocol';
import type { FixtureScenario } from '@/lib/assistant-gateway';

const now = '2026-07-17T12:00:00.000Z';

export const baseConversation: Conversation = {
  id: 'conv-1',
  mode: 'agent',
  title: 'Golden session',
  providerId: 'openai',
  modelId: 'gpt-4o',
  projectId: '/tmp/project',
  permissionProfileId: 'ask',
  createdAt: now,
  updatedAt: now,
};

export const baseRun: Run = {
  id: 'run-template',
  conversationId: 'conv-1',
  status: 'running',
  providerId: 'openai',
  modelId: 'gpt-4o',
  permissionProfile: 'ask',
  startedAt: now,
  lastEventSequence: 0,
};

function ev(
  sequence: number,
  type: string,
  payload: Record<string, unknown> = {},
  runId = 'run-template',
): RunEvent {
  return {
    runId,
    sequence,
    timestamp: now,
    type,
    payload,
  };
}

export const goldenTextStream: FixtureScenario = {
  id: 'text-stream',
  conversations: [baseConversation],
  messagesByConversation: {
    'conv-1': [],
  },
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'text_delta', { text: 'Hello ' }),
      ev(3, 'text_delta', { text: 'world' }),
      ev(4, 'completed', { reason: 'ok' }),
    ],
  },
};

export const goldenReasoning: FixtureScenario = {
  id: 'reasoning',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'reasoning_delta', { text: 'Think step 1. ' }),
      ev(3, 'reasoning_delta', { text: 'Think step 2.' }),
      ev(4, 'text_delta', { text: 'Answer' }),
      ev(5, 'completed', { reason: 'ok' }),
    ],
  },
};

export const goldenTool: FixtureScenario = {
  id: 'tool',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'tool_call_requested', {
        id: 'tc1',
        name: 'read_file',
        input: { path: 'src/a.ts' },
      }),
      ev(3, 'tool_call_started', { id: 'tc1', name: 'read_file' }),
      ev(4, 'tool_call_completed', {
        id: 'tc1',
        name: 'read_file',
        output: { content: 'ok' },
        is_error: false,
        duration_ms: 12,
      }),
      ev(5, 'text_delta', { text: 'Done' }),
      ev(6, 'completed', { reason: 'ok' }),
    ],
  },
};

export const goldenPermission: FixtureScenario = {
  id: 'permission',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'permission_requested', {
        permission_id: 'perm-1',
        tool_call_id: 'tc-write',
        tool_name: 'write_file',
        reason: 'Write to disk',
        input: { path: 'out.txt' },
      }),
      // After respond, fixture can continue — leave incomplete until respond
    ],
  },
};

export const goldenAskUser: FixtureScenario = {
  id: 'ask-user',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'interaction_requested', {
        kind: 'ask_user',
        id: 'ask-1',
        prompt: 'Which approach?',
        options: [
          { id: 'a', label: 'A' },
          { id: 'b', label: 'B' },
        ],
      }),
    ],
  },
};

export const goldenPlanApproval: FixtureScenario = {
  id: 'plan-approval',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'interaction_requested', {
        kind: 'plan_approval',
        id: 'plan-1',
        title: 'Implement feature',
        plan_markdown: '## Steps\n1. A\n2. B',
      }),
    ],
  },
};

export const goldenSubagent: FixtureScenario = {
  id: 'subagent',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'subagent_created', {
        sub_run_id: 'sub-1',
        task: 'Explore codebase',
        agent_profile_id: 'explore',
      }),
      ev(3, 'subagent_completed', {
        sub_run_id: 'sub-1',
        result: 'Found 3 files',
      }),
      ev(4, 'text_delta', { text: 'Summary' }),
      ev(5, 'completed', { reason: 'ok' }),
    ],
  },
};

export const goldenArtifact: FixtureScenario = {
  id: 'artifact',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'artifact_created', {
        id: 'art-1',
        path: '/tmp/out.md',
        label: 'Report',
        kind: 'markdown',
        size: 128,
      }),
      ev(3, 'completed', { reason: 'ok' }),
    ],
  },
  artifactsByRun: {
    // also seed post-hoc via events
  },
};

export const goldenCancelRetry: FixtureScenario = {
  id: 'cancel-retry',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'text_delta', { text: 'partial…' }),
      // cancel injects interrupted via adapter
    ],
  },
};

export const goldenSequenceGap: FixtureScenario = {
  id: 'sequence-gap',
  conversations: [baseConversation],
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'text_delta', { text: 'A' }),
      ev(3, 'text_delta', { text: 'B' }),
      ev(4, 'text_delta', { text: 'C' }),
      ev(5, 'completed', { reason: 'ok' }),
    ],
  },
  // skipSequences filled dynamically with real run id in tests
};

export const goldenReconnect: FixtureScenario = {
  id: 'reconnect',
  conversations: [baseConversation],
  disconnectAfterEvents: 2,
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'text_delta', { text: 'before-drop ' }),
      ev(3, 'text_delta', { text: 'after-drop' }),
      ev(4, 'completed', { reason: 'ok' }),
    ],
  },
};

export const goldenProviderError: FixtureScenario = {
  id: 'provider-error',
  conversations: [baseConversation],
  providerErrorOnStart: true,
  eventsByRun: { __next__: [] },
};

export const goldenPromptQueue: FixtureScenario = {
  id: 'prompt-queue',
  conversations: [baseConversation],
  promptQueues: {
    'conv-1': [],
  },
  eventsByRun: {
    __next__: [
      ev(1, 'started'),
      ev(2, 'text_delta', { text: 'busy' }),
      // stays running until completed — tests enqueue while running
    ],
  },
};

export const ALL_GOLDEN_SCENARIOS: FixtureScenario[] = [
  goldenTextStream,
  goldenReasoning,
  goldenTool,
  goldenPermission,
  goldenAskUser,
  goldenPlanApproval,
  goldenSubagent,
  goldenArtifact,
  goldenCancelRetry,
  goldenSequenceGap,
  goldenReconnect,
  goldenProviderError,
  goldenPromptQueue,
];

export function seedUserMessage(conversationId: string, text: string): Message {
  return {
    id: `user-${conversationId}`,
    conversationId,
    role: 'user',
    status: 'complete',
    createdAt: now,
    contentBlocks: [{ type: 'text', text }],
  };
}
