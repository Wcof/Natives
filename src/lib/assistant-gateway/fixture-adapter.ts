/**
 * FixtureAssistantAdapter — drives GUI from golden scenarios without a daemon.
 */
import type {
  Artifact,
  AssistantMethod,
  Conversation,
  ConversationSnapshot,
  DaemonCapabilities,
  InteractionRequest,
  Message,
  PromptQueueItem,
  Run,
  RunEvent,
} from '@/lib/assistant-protocol';
import type { AssistantGateway } from './gateway';

export interface FixtureScenario {
  id: string;
  conversations?: Conversation[];
  messagesByConversation?: Record<string, Message[]>;
  runs?: Run[];
  /** Events emitted after run.start for a conversation (or global by runId). */
  eventsByRun?: Record<string, RunEvent[]>;
  interactions?: InteractionRequest[];
  promptQueues?: Record<string, PromptQueueItem[]>;
  artifactsByRun?: Record<string, Artifact[]>;
  capabilities?: DaemonCapabilities;
  /** Simulated disconnect after N events on subscribe (for reconnect tests). */
  disconnectAfterEvents?: number;
  /** Sequence gap simulation: skip these sequences on first pass. */
  skipSequences?: Record<string, number[]>;
  providerErrorOnStart?: boolean;
}

const DEFAULT_CAPABILITIES: DaemonCapabilities = {
  protocolVersion: '2.0.0',
  methods: [
    'daemon.getCapabilities',
    'conversation.list',
    'conversation.create',
    'conversation.getMessages',
    'run.start',
    'run.cancel',
    'run.retry',
    'run.getEvents',
    'run.list',
    'run.listChildren',
    'workspace.restorePreview',
    'workspace.restore',
    'permission.respond',
    'permission.listPending',
    'interaction.listPending',
    'interaction.respond',
    'promptQueue.list',
    'promptQueue.enqueue',
    'promptQueue.update',
    'promptQueue.remove',
    'promptQueue.reorder',
    'promptQueue.sendNow',
    'artifact.list',
    'artifact.open',
    'artifact.reveal',
  ],
  providers: ['openai', 'anthropic'],
  tools: true,
  hooks: true,
  subagents: true,
  mcp: true,
  extensions: false,
  scheduler: true,
  eventReplay: true,
  credentialBroker: true,
};

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

export class FixtureAssistantAdapter implements AssistantGateway {
  private connected = false;
  private scenario: FixtureScenario;
  private conversations: Conversation[] = [];
  private messagesByConversation: Record<string, Message[]> = {};
  private runs: Record<string, Run> = {};
  private eventsByRun: Record<string, RunEvent[]> = {};
  private interactions: Record<string, InteractionRequest> = {};
  private promptQueues: Record<string, PromptQueueItem[]> = {};
  private artifactsByRun: Record<string, Artifact[]> = {};
  private seqCounters: Record<string, number> = {};
  private idCounter = 0;
  /** Tracks which sequences already delivered (for gap/replay). */
  private deliveredSequences: Record<string, Set<number>> = {};
  private forceDisconnect = false;

  constructor(scenario: FixtureScenario = { id: 'empty' }) {
    this.scenario = scenario;
    this.resetFromScenario(scenario);
  }

  loadScenario(scenario: FixtureScenario): void {
    this.scenario = scenario;
    this.resetFromScenario(scenario);
  }

  private resetFromScenario(scenario: FixtureScenario): void {
    this.conversations = clone(scenario.conversations ?? []);
    this.messagesByConversation = clone(scenario.messagesByConversation ?? {});
    this.runs = Object.fromEntries((scenario.runs ?? []).map((r) => [r.id, clone(r)]));
    this.eventsByRun = clone(scenario.eventsByRun ?? {});
    this.interactions = Object.fromEntries((scenario.interactions ?? []).map((i) => [i.id, clone(i)]));
    this.promptQueues = clone(scenario.promptQueues ?? {});
    this.artifactsByRun = clone(scenario.artifactsByRun ?? {});
    this.seqCounters = {};
    this.deliveredSequences = {};
    this.forceDisconnect = false;
    for (const [runId, events] of Object.entries(this.eventsByRun)) {
      this.seqCounters[runId] = events.reduce((m, e) => Math.max(m, e.sequence), 0);
    }
  }

  async connect(): Promise<void> {
    this.connected = true;
    this.forceDisconnect = false;
  }

  async disconnect(): Promise<void> {
    this.connected = false;
  }

  /** Test helper: simulate daemon drop. */
  simulateDisconnect(): void {
    this.forceDisconnect = true;
    this.connected = false;
  }

  async getCapabilities(): Promise<DaemonCapabilities | null> {
    return this.scenario.capabilities ?? DEFAULT_CAPABILITIES;
  }

  async request<T>(method: AssistantMethod, params?: unknown): Promise<T> {
    if (!this.connected && method !== 'daemon.ping') {
      throw new Error('Not connected');
    }
    const p = (params ?? {}) as Record<string, unknown>;

    switch (method) {
      case 'daemon.getCapabilities':
        return (await this.getCapabilities()) as T;
      case 'daemon.ping':
        return { ok: true } as T;
      case 'daemon.getStatus':
        return { health: this.connected ? 'healthy' : 'offline' } as T;

      case 'conversation.list': {
        const includeArchived = Boolean(p.include_archived ?? p.includeArchived);
        return this.conversations.filter((c) => includeArchived || !c.archivedAt) as T;
      }
      case 'conversation.create': {
        const id = `conv-${++this.idCounter}`;
        const now = new Date().toISOString();
        const conv: Conversation = {
          id,
          mode: (p.mode as Conversation['mode']) || 'agent',
          title: String(p.title ?? 'New conversation'),
          providerId: String(p.provider_id ?? p.providerId ?? 'openai'),
          modelId: String(p.model_id ?? p.modelId ?? 'gpt-4o'),
          projectId: (p.project_id ?? p.projectId ?? null) as string | null,
          permissionProfileId: String(p.permission_profile_id ?? p.permissionProfileId ?? 'ask'),
          createdAt: now,
          updatedAt: now,
        };
        this.conversations.unshift(conv);
        this.messagesByConversation[id] = [];
        this.promptQueues[id] = [];
        return conv as T;
      }
      case 'conversation.getMessages': {
        const id = String(p.conversation_id ?? p.id ?? p.conversationId);
        return clone(this.messagesByConversation[id] ?? []) as T;
      }
      case 'conversation.rename': {
        const id = String(p.id);
        const title = String(p.title);
        this.conversations = this.conversations.map((c) =>
          c.id === id ? { ...c, title, updatedAt: new Date().toISOString() } : c,
        );
        return { ok: true } as T;
      }
      case 'conversation.archive': {
        const id = String(p.id);
        this.conversations = this.conversations.map((c) =>
          c.id === id ? { ...c, archivedAt: new Date().toISOString() } : c,
        );
        return { ok: true } as T;
      }
      case 'conversation.delete': {
        const id = String(p.id);
        this.conversations = this.conversations.filter((c) => c.id !== id);
        delete this.messagesByConversation[id];
        return { ok: true } as T;
      }
      case 'conversation.fork': {
        const sourceId = String(p.conversation_id ?? p.id);
        const source = this.conversations.find((c) => c.id === sourceId);
        if (!source) throw new Error('conversation not found');
        const id = `conv-fork-${++this.idCounter}`;
        const forked: Conversation = {
          ...clone(source),
          id,
          title: `${source.title} (fork)`,
          parentConversationId: sourceId,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        };
        this.conversations.unshift(forked);
        this.messagesByConversation[id] = clone(this.messagesByConversation[sourceId] ?? []);
        return forked as T;
      }
      case 'conversation.update_model': {
        const id = String(p.id);
        this.conversations = this.conversations.map((c) =>
          c.id === id
            ? {
                ...c,
                providerId: String(p.provider_id ?? p.providerId ?? c.providerId),
                modelId: String(p.model_id ?? p.modelId ?? c.modelId),
                updatedAt: new Date().toISOString(),
              }
            : c,
        );
        return { updated_at: new Date().toISOString() } as T;
      }
      case 'conversation.update_permission': {
        const id = String(p.id);
        this.conversations = this.conversations.map((c) =>
          c.id === id
            ? {
                ...c,
                permissionProfileId: String(p.permission_profile_id ?? p.permissionProfileId ?? 'ask'),
              }
            : c,
        );
        return { ok: true } as T;
      }

      case 'run.start': {
        if (this.scenario.providerErrorOnStart) {
          const err = new Error('Provider rejected request');
          (err as Error & { code?: string }).code = 'provider_error';
          throw err;
        }
        const conversationId = String(p.conversation_id ?? p.conversationId);
        const content = p.content != null ? String(p.content) : undefined;
        const runId = `run-${++this.idCounter}`;
        const now = new Date().toISOString();
        if (content) {
          const userMsg: Message = {
            id: `msg-user-${this.idCounter}`,
            conversationId,
            role: 'user',
            status: 'complete',
            createdAt: now,
            contentBlocks: [{ type: 'text', text: content }],
          };
          this.messagesByConversation[conversationId] = [
            ...(this.messagesByConversation[conversationId] ?? []),
            userMsg,
          ];
        }
        const run: Run = {
          id: runId,
          conversationId,
          status: 'running',
          providerId: String(p.provider_id ?? p.providerId ?? 'openai'),
          modelId: String(p.model_id ?? p.modelId ?? 'gpt-4o'),
          permissionProfile: String(p.permission_profile ?? p.permissionProfile ?? 'ask'),
          startedAt: now,
          lastEventSequence: 0,
        };
        this.runs[runId] = run;

        // Attach pre-scripted events for this scenario under the new run id if
        // events were keyed by conversation or a template run.
        const template =
          this.eventsByRun['__next__'] ??
          this.eventsByRun[conversationId] ??
          this.eventsByRun['default'];
        if (template && !this.eventsByRun[runId]) {
          this.eventsByRun[runId] = template.map((e) => ({
            ...clone(e),
            runId,
          }));
        }
        if (!this.eventsByRun[runId]) {
          // Minimal text completion when no scripted events
          this.eventsByRun[runId] = [
            { runId, sequence: 1, timestamp: now, type: 'started', payload: {} },
            {
              runId,
              sequence: 2,
              timestamp: now,
              type: 'text_delta',
              payload: { text: 'Fixture reply.' },
            },
            {
              runId,
              sequence: 3,
              timestamp: now,
              type: 'completed',
              payload: { reason: 'ok' },
            },
          ];
        }
        this.seqCounters[runId] = (this.eventsByRun[runId] ?? []).reduce(
          (m, e) => Math.max(m, e.sequence),
          0,
        );
        return clone(run) as T;
      }
      case 'run.cancel': {
        const runId = String(p.run_id ?? p.runId);
        const run = this.runs[runId];
        if (run && run.status !== 'completed') {
          this.runs[runId] = {
            ...run,
            status: 'cancelling',
          };
          const seq = (this.seqCounters[runId] ?? 0) + 1;
          this.seqCounters[runId] = seq;
          const events = this.eventsByRun[runId] ?? [];
          events.push({
            runId,
            sequence: seq,
            timestamp: new Date().toISOString(),
            type: 'interrupted',
            payload: { reason: 'user_cancel' },
          });
          this.eventsByRun[runId] = events;
          this.runs[runId] = { ...this.runs[runId]!, status: 'interrupted', finishedAt: new Date().toISOString() };
        }
        return { ok: true } as T;
      }
      case 'run.retry': {
        const oldId = String(p.run_id ?? p.runId);
        const old = this.runs[oldId];
        if (!old) throw new Error('run not found');
        return this.request('run.start', {
          conversation_id: old.conversationId,
          provider_id: old.providerId,
          model_id: old.modelId,
        });
      }
      case 'run.list': {
        const conversationId = p.conversation_id ?? p.conversationId;
        let list = Object.values(this.runs);
        if (conversationId) list = list.filter((r) => r.conversationId === String(conversationId));
        const limit = Number(p.limit ?? 50);
        return clone(list.slice(0, limit)) as T;
      }
      case 'run.getEvents': {
        const runId = String(p.run_id ?? p.runId);
        const after = Number(p.after_sequence ?? p.afterSequence ?? 0);
        const events = (this.eventsByRun[runId] ?? []).filter((e) => e.sequence > after);
        return clone(events) as T;
      }
      case 'run.listChildren': {
        const parent = String(p.parent_run_id ?? p.parentRunId);
        const children = Object.values(this.runs)
          .filter((r) => r.parentRunId === parent)
          .map((r) => ({
            id: r.id,
            parentRunId: parent,
            status: r.status,
            task: r.activity ?? 'subagent',
            providerId: r.providerId,
            modelId: r.modelId,
          }));
        return clone(children) as T;
      }
      case 'workspace.restore': {
        return { ok: true, rewound: true } as T;
      }
      case 'workspace.restorePreview': {
        return {
          checkpoint_id: `fixture-checkpoint-${String(p.run_id ?? 'run')}`,
          files: [],
          conflicts: [],
        } as T;
      }

      case 'permission.respond':
      case 'interaction.respond': {
        const id = String(p.request_id ?? p.permission_id ?? p.id ?? p.interaction_id);
        delete this.interactions[id];
        // Also clear from any run's pending via permission_responded event if run known
        const runId = p.run_id != null ? String(p.run_id) : undefined;
        if (runId) {
          const seq = (this.seqCounters[runId] ?? 0) + 1;
          this.seqCounters[runId] = seq;
          const events = this.eventsByRun[runId] ?? [];
          events.push({
            runId,
            sequence: seq,
            timestamp: new Date().toISOString(),
            type: 'permission_responded',
            payload: {
              permission_id: id,
              approved: Boolean(p.approved),
              scope: String(p.scope ?? 'once'),
            },
          });
          this.eventsByRun[runId] = events;
        }
        return { ok: true } as T;
      }
      case 'permission.listPending':
      case 'interaction.listPending':
        return Object.values(this.interactions) as T;

      case 'promptQueue.list': {
        const cid = String(p.conversation_id ?? p.conversationId);
        return clone(this.promptQueues[cid] ?? []) as T;
      }
      case 'promptQueue.enqueue': {
        const cid = String(p.conversation_id ?? p.conversationId);
        const clientTempId = p.client_temp_id != null || p.clientTempId != null
          ? String(p.client_temp_id ?? p.clientTempId)
          : undefined;
        const queue = this.promptQueues[cid] ?? [];
        const item: PromptQueueItem = {
          id: `pq-${++this.idCounter}`,
          conversationId: cid,
          content: String(p.content ?? ''),
          source: (p.source as PromptQueueItem['source']) || 'user',
          createdAt: new Date().toISOString(),
          order: queue.length,
          clientTempId,
        };
        this.promptQueues[cid] = [...queue, item];
        return clone(item) as T;
      }
      case 'promptQueue.update': {
        const id = String(p.id);
        const cid = String(p.conversation_id ?? p.conversationId ?? '');
        const queues = cid
          ? { [cid]: this.promptQueues[cid] ?? [] }
          : this.promptQueues;
        for (const [key, items] of Object.entries(queues)) {
          this.promptQueues[key] = items.map((it) =>
            it.id === id ? { ...it, content: String(p.content ?? it.content) } : it,
          );
        }
        return { ok: true } as T;
      }
      case 'promptQueue.remove': {
        const id = String(p.id);
        for (const key of Object.keys(this.promptQueues)) {
          this.promptQueues[key] = (this.promptQueues[key] ?? []).filter((it) => it.id !== id);
        }
        return { ok: true } as T;
      }
      case 'promptQueue.reorder': {
        const cid = String(p.conversation_id ?? p.conversationId);
        const order = (p.ids as string[]) ?? [];
        const items = this.promptQueues[cid] ?? [];
        const map = new Map(items.map((i) => [i.id, i]));
        this.promptQueues[cid] = order
          .map((id, index) => {
            const it = map.get(id);
            return it ? { ...it, order: index } : null;
          })
          .filter(Boolean) as PromptQueueItem[];
        return { ok: true } as T;
      }
      case 'promptQueue.sendNow': {
        const id = String(p.id);
        // Move to front
        for (const key of Object.keys(this.promptQueues)) {
          const items = this.promptQueues[key] ?? [];
          const idx = items.findIndex((i) => i.id === id);
          if (idx >= 0) {
            const [item] = items.splice(idx, 1);
            items.unshift({ ...item!, order: 0 });
            this.promptQueues[key] = items.map((it, i) => ({ ...it, order: i }));
          }
        }
        return { ok: true } as T;
      }

      case 'artifact.list': {
        const runId = String(p.run_id ?? p.runId ?? '');
        if (runId) return clone(this.artifactsByRun[runId] ?? []) as T;
        return clone(Object.values(this.artifactsByRun).flat()) as T;
      }
      case 'artifact.open':
      case 'artifact.reveal':
        return { ok: true, path: String(p.path ?? '') } as T;

      case 'provider.list':
        return [
          {
            id: 'openai',
            provider_type: 'openai',
            display_name: 'OpenAI',
            has_active_key: true,
            models: [{ id: 'gpt-4o', display_name: 'GPT-4o' }],
          },
        ] as T;

      default:
        throw new Error(`Fixture method unsupported: ${method}`);
    }
  }

  async *subscribe(runId: string, afterSequence: number): AsyncIterable<RunEvent> {
    const events = this.eventsByRun[runId] ?? [];
    const skip = new Set(this.scenario.skipSequences?.[runId] ?? []);
    let emitted = 0;
    if (!this.deliveredSequences[runId]) this.deliveredSequences[runId] = new Set();

    for (const event of events) {
      if (event.sequence <= afterSequence) continue;
      // First pass: simulate gap by skipping sequences
      if (skip.has(event.sequence) && !this.deliveredSequences[runId]!.has(event.sequence)) {
        // leave a gap — don't mark delivered so replay can fill
        continue;
      }
      if (this.forceDisconnect || !this.connected) {
        throw new Error('disconnected');
      }
      if (
        this.scenario.disconnectAfterEvents != null &&
        emitted >= this.scenario.disconnectAfterEvents
      ) {
        this.forceDisconnect = true;
        this.connected = false;
        throw new Error('disconnected');
      }
      this.deliveredSequences[runId]!.add(event.sequence);
      emitted += 1;
      yield clone(event);
      // yield control so callers can batch
      await Promise.resolve();
    }
  }

  /** Replay path: clear skip list and return missing events. */
  async replay(runId: string, afterSequence: number): Promise<RunEvent[]> {
    // On replay, do not skip
    const events = (this.eventsByRun[runId] ?? []).filter((e) => e.sequence > afterSequence);
    for (const e of events) {
      this.deliveredSequences[runId] ??= new Set();
      this.deliveredSequences[runId]!.add(e.sequence);
    }
    return clone(events);
  }

  async getSnapshot(conversationId: string): Promise<ConversationSnapshot> {
    const conversation = this.conversations.find((c) => c.id === conversationId);
    if (!conversation) {
      throw new Error(`conversation not found: ${conversationId}`);
    }
    const runs = Object.values(this.runs).filter((r) => r.conversationId === conversationId);
    const active = runs.find((r) => r.status === 'running' || r.status === 'waiting_permission') ?? runs[0];
    const eventsByRun: Record<string, RunEvent[]> = {};
    for (const r of runs) {
      eventsByRun[r.id] = clone(this.eventsByRun[r.id] ?? []);
    }
    return {
      conversation: clone(conversation),
      messages: clone(this.messagesByConversation[conversationId] ?? []),
      runs: clone(runs),
      activeRunId: active?.id ?? null,
      eventsByRun,
      interactions: Object.values(this.interactions).filter(
        (i) => !i.conversationId || i.conversationId === conversationId,
      ),
      promptQueue: clone(this.promptQueues[conversationId] ?? []),
      artifacts: runs.flatMap((r) => this.artifactsByRun[r.id] ?? []),
      children: Object.values(this.runs)
        .filter((r) => r.parentRunId && runs.some((p) => p.id === r.parentRunId))
        .map((r) => ({
          id: r.id,
          parentRunId: r.parentRunId!,
          status: r.status,
          task: r.activity ?? 'subagent',
          providerId: r.providerId,
          modelId: r.modelId,
        })),
      capabilities: await this.getCapabilities(),
    };
  }
}
