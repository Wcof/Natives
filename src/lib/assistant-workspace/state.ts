import type {
  Artifact,
  BackgroundTask,
  CapabilitySelection,
  ChildRunSummary,
  ConnectionState,
  ContentBlock,
  ContextUsage,
  Conversation,
  ConversationSnapshot,
  DaemonCapabilities,
  FileChange,
  InteractionRequest,
  Message,
  PromptQueueItem,
  Run,
  RunEvent,
} from '@/lib/assistant-protocol';

export interface ComposerDraft {
  text: string;
  attachments: Array<{ path: string; name: string; mimeType?: string; size?: number }>;
  updatedAt: string;
}

export type InspectorTab = 'run' | 'tasks' | 'changes' | 'artifacts' | 'context' | 'events';

export interface AssistantViewState {
  leftCollapsed: boolean;
  rightCollapsed: boolean;
  leftWidth: number;
  rightWidth: number;
  inspectorTab: InspectorTab;
  /** Per-conversation scroll/follow */
  followTailByConversation: Record<string, boolean>;
  blockExpanded: Record<string, boolean>;
  layoutBreakpoint: 'full' | 'drawer-right' | 'drawer-both';
}

export interface LiveBubble {
  runId: string;
  conversationId: string;
  messageId: string;
  blocks: ContentBlock[];
  reasoningStartedAt: string | null;
  reasoningFinishedAt: string | null;
  attemptSnapshots?: Record<number, ContentBlock[]>;
}

export interface AssistantWorkspaceState {
  connection: ConnectionState;
  connectionError: string | null;
  reconnectAttempts: number;
  capabilities: DaemonCapabilities | null;

  conversations: Record<string, Conversation>;
  conversationOrder: string[];
  activeConversationId: string | null;

  messagesByConversation: Record<string, string[]>;
  messages: Record<string, Message>;
  messagePageInfoByConversation: Record<string, { hasMore: boolean; nextCursor: { createdAt: string; id: string } | null }>;

  runs: Record<string, Run>;
  /** Active (latest) run per conversation */
  activeRunByConversation: Record<string, string | null>;
  eventsByRun: Record<string, RunEvent[]>;
  lastSequenceByRun: Record<string, number>;
  /** Runs currently recovering from sequence gap */
  recoveringRuns: Record<string, boolean>;

  interactions: Record<string, InteractionRequest>;
  /** Ordered interaction ids waiting for user */
  interactionOrder: string[];

  promptQueues: Record<string, PromptQueueItem[]>;

  childRunsByParent: Record<string, string[]>;
  childSummaries: Record<string, ChildRunSummary>;
  tasks: Record<string, BackgroundTask>;
  artifactsByRun: Record<string, Artifact[]>;
  fileChangesByRun: Record<string, FileChange[]>;
  contextUsageByConversation: Record<string, ContextUsage>;

  composerByConversation: Record<string, ComposerDraft>;
  /** ADR-0016 conversation-level capability selection; null/absent = legacy behaviour. */
  capabilitySelectionByConversation: Record<string, CapabilitySelection | null>;
  liveByRun: Record<string, LiveBubble>;
  view: AssistantViewState;
}

export function createInitialWorkspaceState(): AssistantWorkspaceState {
  return {
    connection: 'disconnected',
    connectionError: null,
    reconnectAttempts: 0,
    capabilities: null,
    conversations: {},
    conversationOrder: [],
    activeConversationId: null,
    messagesByConversation: {},
    messages: {},
    messagePageInfoByConversation: {},
    runs: {},
    activeRunByConversation: {},
    eventsByRun: {},
    lastSequenceByRun: {},
    recoveringRuns: {},
    interactions: {},
    interactionOrder: [],
    promptQueues: {},
    childRunsByParent: {},
    childSummaries: {},
    tasks: {},
    artifactsByRun: {},
    fileChangesByRun: {},
    contextUsageByConversation: {},
    composerByConversation: {},
    capabilitySelectionByConversation: {},
    liveByRun: {},
    view: {
      leftCollapsed: false,
      rightCollapsed: false,
      leftWidth: 260,
      rightWidth: 320,
      inspectorTab: 'run',
      followTailByConversation: {},
      blockExpanded: {},
      layoutBreakpoint: 'full',
    },
  };
}

export type WorkspaceAction =
  | { type: 'connection/set'; connection: ConnectionState; error?: string | null; reconnectAttempts?: number }
  | { type: 'capabilities/set'; capabilities: DaemonCapabilities | null }
  | { type: 'conversations/replace'; conversations: Conversation[] }
  | { type: 'conversations/upsert'; conversation: Conversation }
  | { type: 'conversations/remove'; id: string }
  | { type: 'conversations/setActive'; id: string | null }
  | { type: 'snapshot/apply'; snapshot: ConversationSnapshot }
  | { type: 'run/upsert'; run: Run }
  | { type: 'event/apply'; event: RunEvent }
  | { type: 'event/applyBatch'; events: RunEvent[] }
  | { type: 'event/replay'; runId: string; events: RunEvent[] }
  | { type: 'recovering/set'; runId: string; recovering: boolean }
  | { type: 'interaction/upsert'; interaction: InteractionRequest }
  | { type: 'interaction/remove'; id: string }
  | { type: 'promptQueue/set'; conversationId: string; items: PromptQueueItem[] }
  | { type: 'promptQueue/optimisticEnqueue'; item: PromptQueueItem }
  | { type: 'promptQueue/reassociate'; conversationId: string; clientTempId: string; serverItem: PromptQueueItem }
  | { type: 'composer/set'; conversationId: string; draft: Partial<ComposerDraft> }
  | { type: 'composer/clear'; conversationId: string }
  | { type: 'capabilitySelection/set'; conversationId: string; selection: CapabilitySelection | null }
  | { type: 'view/patch'; patch: Partial<AssistantViewState> }
  | { type: 'view/setBlockExpanded'; key: string; expanded: boolean }
  | { type: 'messages/appendOptimistic'; message: Message }
  /** Drop a stuck optimistic user bubble / clear live run after send failure. */
  | { type: 'messages/remove'; id: string; conversationId: string }
  | { type: 'messages/prependPage'; conversationId: string; messages: Message[]; pageInfo: { hasMore: boolean; nextCursor: { createdAt: string; id: string } | null } }
  | { type: 'run/clearActive'; conversationId: string; runId?: string }
  | { type: 'disconnect/soft' };
