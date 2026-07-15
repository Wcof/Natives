'use client';

// ─── New Assistant Workbench ─────────────────────────────
//
// Center conversation surface. Shell-level navigation and context panels consume
// the same workspace snapshot through AssistantWorkspaceContext.

import { useState, useCallback, useEffect, useMemo, useRef } from 'react';
import { t, type Locale } from '@/i18n';
import ConversationTimeline from './ConversationTimeline';
import type { ContentBlock } from './blocks';
import MessageInput from './MessageInput';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { Loader2, WifiOff } from 'lucide-react';
import {
  classifyProviderReadiness,
  selectAssistantModel,
  type ProviderReadiness,
} from '@/lib/provider-model-selection';
import type { ProviderWithModels } from './ModelSelectorDropdown';
import { readActiveProject, writeActiveProject } from '@/lib/active-project';
import {
  classifyAssistantSurface,
  groupAssistantConversations,
  projectCreationState,
} from '@/lib/assistant-project-groups';
import {
  createAssistantStreamState,
  reduceAssistantStreamEvent,
  type AssistantStreamState,
} from '@/lib/assistant-stream-state';
import type { AssistantRunEvent } from '@/lib/assistant-types';
import { useAssistantWorkspace, type AssistantWorkspaceActions } from './AssistantWorkspaceContext';
import RunInspector from './RunInspector';
import PermissionRequestCard from './PermissionRequestCard';
import { useAssistantStream } from './hooks/useAssistantStream';
import type { ProjectSummary, ProviderSummary } from '@/lib/tauri-adapter';
import { normalizePermissionProfile, type AssistantDraft, type AssistantPermissionProfile } from '@/lib/assistant-composer';
import { PanelRightClose, PanelRightOpen } from 'lucide-react';

// ─── Types ───────────────────────────────────────────────

interface Conversation {
  id: string;
  mode: 'chat' | 'agent';
  project_id?: string | null;
  title: string;
  provider_id: string;
  model_id: string;
  permission_profile_id?: AssistantPermissionProfile;
  created_at: string;
  updated_at: string;
  archived_at?: string | null;
}

interface MessageBlock {
  type: string;
  index: number;
  content: unknown;
}

interface Message {
  id: string;
  conversation_id: string;
  parent_message_id?: string | null;
  role: 'system' | 'user' | 'assistant';
  status: string;
  input_tokens?: number;
  output_tokens?: number;
  created_at: string;
  content_blocks: MessageBlock[];
}

interface RunEvent {
  sequence: number;
  timestamp: string;
  type: string;
  payload: unknown;
}

interface Run {
  id: string;
  conversation_id: string;
  status: string;
  provider_id: string;
  model_id: string;
  started_at?: string | null;
  finished_at?: string | null;
  error_code?: string | null;
  step_count?: number;
}

interface Artifact {
  id: string;
  label?: string;
  path: string;
  kind: string;
  size: number;
}

interface ProviderInfo {
  id: string;
  provider_type: string;
  display_name: string;
  api_base_url: string;
  health_status: string;
  default_model?: string | null;
  has_active_key?: boolean;
  models?: ModelInfo[];
}

interface ModelInfo {
  id: string;
  display_name?: string;
  context_window?: number;
  capabilities?: {
    streaming?: boolean;
    tool_calling?: boolean;
    image_input?: boolean;
    reasoning?: boolean;
  };
}

// ─── Props ───────────────────────────────────────────────

interface AssistantWorkbenchProps {
  locale: Locale;
}

// ─── Helpers ─────────────────────────────────────────────

function v2call<T = unknown>(method: string, params?: unknown): Promise<T> {
  if (!window.nativesAPI?.assistantV2) {
    return Promise.reject(new Error('assistantV2 not available'));
  }
  return window.nativesAPI.assistantV2.request(method, params) as Promise<T>;
}

function mapBlockToContentBlock(block: MessageBlock): ContentBlock {
  const content = (block.content ?? {}) as Record<string, unknown>;

  switch (block.type) {
    case 'text':
      return { type: 'text', text: String(content.text ?? content.content ?? '') };
    case 'reasoning':
      return { type: 'reasoning', reasoning: String(content.text ?? content.reasoning ?? content.content ?? ''), signature: content.signature ? String(content.signature) : undefined };
    case 'image':
      return { type: 'image', mimeType: String(content.mime_type ?? 'image/png'), imageUrl: String(content.data ?? ''), altText: String(content.alt_text ?? '') };
    case 'file_reference':
      return { type: 'file_reference', filePath: String(content.path ?? content.file_path ?? ''), mimeType: String(content.mime_type ?? 'application/octet-stream'), fileSize: Number(content.size ?? content.file_size ?? 0) };
    case 'tool_call':
      return {
        type: 'tool_call',
        toolCallId: String(content.tool_call_id ?? content.id ?? ''),
        toolName: String(content.tool_name ?? content.name ?? ''),
        toolInput: (content.input ?? content.arguments ?? {}) as Record<string, unknown>,
        toolStatus: (content.status ?? 'pending') as 'pending' | 'running' | 'completed' | 'failed' | 'rejected',
      };
    case 'tool_result':
      return {
        type: 'tool_result',
        toolCallId: String(content.tool_call_id ?? ''),
        toolOutput: content.output ?? content.result,
        isError: Boolean(content.is_error ?? false),
        durationMs: content.duration_ms == null ? undefined : Number(content.duration_ms),
      };
    case 'citation':
      return {
        type: 'citation',
        citationUri: String(content.uri ?? content.url ?? ''),
        citationTitle: content.title ? String(content.title) : undefined,
      };
    case 'error':
      return {
        type: 'error',
        errorCode: String(content.code ?? content.error_code ?? ''),
        errorMessage: String(content.message ?? content.error_message ?? ''),
        retryable: Boolean(content.retryable ?? false),
      };
    default:
      return {
        type: 'legacy',
        raw: JSON.stringify(content, null, 2),
        originalType: block.type,
      };
  }
}

function mapMessageToTimeline(msg: Message): import('./ConversationTimeline').Message {
  return {
    id: msg.id,
    role: msg.role,
    contentBlocks: (msg.content_blocks ?? []).map(mapBlockToContentBlock),
    status: msg.status,
    createdAt: msg.created_at,
    inputTokens: msg.input_tokens,
    outputTokens: msg.output_tokens,
  };
}

// ─── Component ───────────────────────────────────────────

export default function AssistantWorkbench({ locale }: AssistantWorkbenchProps) {
  const { toast } = useToast();
  const { publishNavigation, publishRuntime, registerActions } = useAssistantWorkspace();

  // ── Connection state ──
  const [daemonConnected, setDaemonConnected] = useState(false);
  const [daemonError, setDaemonError] = useState<string | null>(null);
  const [rendererOnly, setRendererOnly] = useState(false);

  // ── Data state ──
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeConversationId, setActiveConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [providerLoadError, setProviderLoadError] = useState<string | null>(null);
  const [providerReadiness, setProviderReadiness] = useState<ProviderReadiness>('no_provider');
  const [loadingConversations, setLoadingConversations] = useState(true);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [activeProjectPath, setActiveProjectPath] = useState<string | null>(null);
  const [registeredProjects, setRegisteredProjects] = useState<ProjectSummary[]>([]);
  const [rightPanelOpen, setRightPanelOpen] = useState(true);
  const [respondedPermissionId, setRespondedPermissionId] = useState<string | null>(null);

  // ── Run state (unified via AssistantStreamState) ──
  const [streamState, setStreamState] = useState<AssistantStreamState | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  // Derived from streamState, kept for backward compat with RunInspector & publishRuntime
  const [runEvents, setRunEvents] = useState<RunEvent[]>([]);
  const [activeRunStartedAt, setActiveRunStartedAt] = useState<string | null>(null);
  const [activeRunFinishedAt, setActiveRunFinishedAt] = useState<string | null>(null);

  // ── UI state ──
  const [inputDisabledReason, setInputDisabledReason] = useState<'no_provider' | 'no_model' | 'creating' | null>(null);
  const [isCreatingConversation, setIsCreatingConversation] = useState(false);

  // Refs
  const streamStateRef = useRef<AssistantStreamState | null>(null);
  const latestRunIdRef = useRef<string | null>(null);
  const activeRunIdRef = useRef<string | null>(null);
  const eventsRef = useRef<RunEvent[]>([]);
  const batchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const persistedStreamRunRef = useRef<string | null>(null);
  const { streamState: nativeStream, resetStream } = useAssistantStream({ sessionId: activeConversationId, locale });

  const checkDaemonStatus = useCallback(async (): Promise<boolean> => {
    try {
      const assistantApi = window.nativesAPI?.assistantV2;
      if (!assistantApi) {
        setRendererOnly(true);
        setDaemonConnected(false);
        setDaemonError(t(locale, 'assistant.desktopEngineRequired'));
        return false;
      }
      setRendererOnly(false);
      const status = await assistantApi.getStatus();
      const connected = status?.connected === true;
      setDaemonConnected(connected);
      setDaemonError(connected ? null : (status?.error ?? t(locale, 'assistant.daemonNotConnected')));
      return connected;
    } catch (error) {
      setDaemonConnected(false);
      setDaemonError(classifyError(error).userMessage);
      return false;
    }
  }, [locale]);

  // ── Check daemon status on mount ──
  useEffect(() => {
    let cancelled = false;
    let retryTimer: number | undefined;
    const startupTimer = window.setTimeout(() => {
      void checkDaemonStatus().then(connected => {
        if (cancelled || connected) return;
        // The supervisor may still be completing its startup handshake.
        retryTimer = window.setTimeout(() => {
          if (!cancelled) void checkDaemonStatus();
        }, 750);
      });
    }, 0);

    return () => {
      cancelled = true;
      window.clearTimeout(startupTimer);
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [checkDaemonStatus]);

  // ── Load providers ──
  const loadProviders = async () => {
    try {
      const result = await window.nativesAPI?.provider.list();
      if (!result) throw new Error('Provider API unavailable');
      const nextProviders = result.map((provider: ProviderSummary): ProviderInfo => ({
        id: provider.id,
        provider_type: provider.providerType,
        display_name: provider.displayName,
        api_base_url: provider.baseUrl,
        health_status: 'unknown',
        default_model: provider.defaultModel,
        has_active_key: provider.keys.some(key => key.isActive && key.status === 'valid'),
        models: provider.defaultModel ? [{ id: provider.defaultModel }] : [],
      }));
      const readiness = classifyProviderReadiness(nextProviders);
      setProviders(nextProviders);
      setProviderReadiness(readiness);
      setProviderLoadError(null);
      setInputDisabledReason(readiness === 'ready' ? null : readiness);
    } catch (error) {
      const message = classifyError(error).userMessage;
      setProviderLoadError(message);
      toast(message, 'error');
    }
  };

  // ── Load conversations ──
  const loadConversations = async () => {
    setLoadingConversations(true);
    try {
      const result = await v2call<Conversation[]>('conversation.list', { include_archived: false });
      setConversations(result.filter(conversation => !conversation.archived_at));
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setLoadingConversations(false);
    }
  };

  const loadProjects = useCallback(async () => {
    const projects = await window.nativesAPI?.project.list();
    setRegisteredProjects(projects ?? []);
  }, []);

  // ── Load data when connected ──
  useEffect(() => {
    if (!daemonConnected) return;
    let cancelled = false;

    (async () => {
      const projPath = await readActiveProject(window.nativesAPI);
      if (cancelled) return;
      setActiveProjectPath(projPath);
      await Promise.all([loadProviders(), loadConversations(), loadProjects()]);
    })();

    return () => { cancelled = true; };
    // These loaders intentionally run once for each daemon connection.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [daemonConnected, loadProjects]);

  // ── Load messages ──
  const loadMessages = useCallback(async (conversationId: string) => {
    setLoadingMessages(true);
    try {
      const result = await v2call<Message[]>('conversation.getMessages', {
        conversation_id: conversationId,
      });
      setMessages(result);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally {
      setLoadingMessages(false);
    }
  }, [toast]);

  // ── Load artifacts ──
  const loadArtifacts = useCallback(async (runId: string) => {
    try {
      const result = await v2call<Artifact[]>('artifact.list', { run_id: runId });
      setArtifacts(result);
    } catch {
      setArtifacts([]);
    }
  }, []);

  const loadConversationRun = useCallback(async (conversationId: string) => {
    try {
      const result = await v2call<Run[]>('run.list', { conversation_id: conversationId, limit: 1 });
      const run = result[0];
      if (!run) return;
      latestRunIdRef.current = run.id;
      activeRunIdRef.current = run.id;
      setActiveRunStartedAt(run.started_at ?? null);
      setActiveRunFinishedAt(run.finished_at ?? null);
      const [eventResult] = await Promise.all([
        v2call<RunEvent[]>('run.getEvents', { run_id: run.id, after_sequence: 0 }),
        loadArtifacts(run.id),
      ]);
      const restoredEvents = eventResult;
      let restored = createAssistantStreamState(run.id);
      for (const event of restoredEvents) {
        restored = reduceAssistantStreamEvent(restored, {
          runId: run.id,
          sequence: event.sequence,
          timestamp: event.timestamp,
          type: event.type as AssistantRunEvent['type'],
          payload: (event.payload ?? {}) as Record<string, unknown>,
        });
      }
      // Apply the persisted run status as the final state override
      if (run.status === 'completed' || run.status === 'failed' || run.status === 'interrupted') {
        restored = { ...restored, status: run.status };
      }
      streamStateRef.current = restored;
      setStreamState(restored);
      // Rebuild events from the restored state for the RunInspector
      eventsRef.current = restoredEvents;
      setRunEvents(restoredEvents);
    } catch {
      // No run for this conversation — that's fine
    }
  }, [loadArtifacts]);

  // ── Handle stream event ──
  const handleStreamEvent = useCallback((event: { runId: string; sequence: number; timestamp?: string; type: string; payload: unknown }) => {
    const previous = streamStateRef.current?.runId === event.runId
      ? streamStateRef.current
      : createAssistantStreamState(event.runId);
    const next = reduceAssistantStreamEvent(previous, {
      runId: event.runId,
      sequence: event.sequence,
      timestamp: event.timestamp ?? new Date().toISOString(),
      type: event.type as AssistantRunEvent['type'],
      payload: (event.payload ?? {}) as Record<string, unknown>,
    });
    if (next === previous) return;
    streamStateRef.current = next;

    const runEvent: RunEvent = {
      sequence: event.sequence,
      timestamp: event.timestamp ?? new Date().toISOString(),
      type: event.type,
      payload: event.payload,
    };

    // Accumulate events for RunInspector
    eventsRef.current = [...eventsRef.current, runEvent];

    // Batch update with debounce to avoid thrashing
    if (batchTimerRef.current) {
      clearTimeout(batchTimerRef.current);
    }
    batchTimerRef.current = setTimeout(() => {
      setStreamState({ ...streamStateRef.current! });
      setRunEvents([...eventsRef.current]);
    }, 30);

    // Check if this is for the active run
    if (event.runId === latestRunIdRef.current) {
      const terminalEvent = event.type === 'completed' || event.type === 'failed' || event.type === 'interrupted';
      if (!terminalEvent && event.type !== 'usage_updated') setIsStreaming(true);

      if (terminalEvent) {
        setIsStreaming(false);
        setActiveRunFinishedAt(event.timestamp ?? new Date().toISOString());
        // Reload messages to get the final persisted state
        if (activeConversationId) {
          void loadMessages(activeConversationId).finally(() => {
            // Stream state remains as the live rendering until persisted messages arrive
          });
        }
      }
    }
  }, [activeConversationId, loadMessages]);

  // The runtime streams over Tauri events. Mirror it into the workbench while it
  // runs, then persist the completed answer so reopening the session is stable.
  useEffect(() => {
    const runId = activeRunIdRef.current;
    if (!runId || !activeConversationId || !isStreaming) return;

    if (nativeStream.content || nativeStream.reasoning || nativeStream.toolCall || nativeStream.toolEvents.length > 0) {
      const blocks: import('./blocks').ContentBlock[] = [];
      if (nativeStream.reasoning) blocks.push({ type: 'reasoning', reasoning: nativeStream.reasoning });
      for (const tool of nativeStream.toolEvents) {
        blocks.push({ type: 'tool_call', toolCallId: tool.id, toolName: tool.name, toolInput: tool.input, toolStatus: tool.status });
        if (tool.output !== undefined) blocks.push({ type: 'tool_result', toolCallId: tool.id, toolOutput: tool.output, isError: tool.status === 'failed' || tool.status === 'rejected' });
      }
      if (nativeStream.toolCall) blocks.push({ type: 'tool_call', toolCallId: 'active-tool', toolName: nativeStream.toolCall, toolInput: {}, toolStatus: nativeStream.toolStatus === 'error' ? 'failed' : nativeStream.done ? 'completed' : 'running' });
      if (nativeStream.toolResult !== null) blocks.push({ type: 'tool_result', toolCallId: 'active-tool', toolOutput: nativeStream.toolResult, isError: nativeStream.toolStatus === 'error' });
      if (nativeStream.content) blocks.push({ type: 'text', text: nativeStream.content });
      const next = {
        runId,
        status: nativeStream.done ? (nativeStream.error ? 'failed' : 'completed') : 'running',
        lastSequence: streamStateRef.current?.lastSequence ?? 0,
        blocks,
        fileChanges: [],
        usage: { inputTokens: null, outputTokens: null, reasoningTokens: null },
      };
      streamStateRef.current = next;
      setStreamState(next);
    }

    if (!nativeStream.done || persistedStreamRunRef.current === runId) return;
    persistedStreamRunRef.current = runId;
    const status = nativeStream.error ? 'failed' : 'completed';
    setIsStreaming(false);
    setActiveRunFinishedAt(new Date().toISOString());
    const persistedBlocks = [
      ...(nativeStream.reasoning ? [{ type: 'reasoning', reasoning: nativeStream.reasoning }] : []),
      ...nativeStream.toolEvents.flatMap(tool => [
        { type: 'tool_call', tool_call_id: tool.id, tool_name: tool.name, input: tool.input, status: tool.status },
        ...(tool.output === undefined ? [] : [{ type: 'tool_result', tool_call_id: tool.id, output: tool.output, is_error: tool.status === 'failed' || tool.status === 'rejected' }]),
      ]),
      ...(nativeStream.content ? [{ type: 'text', text: nativeStream.content }] : []),
    ];
    void Promise.all([
      persistedBlocks.length > 0
        ? v2call('conversation.appendMessage', { conversation_id: activeConversationId, role: 'assistant', blocks: persistedBlocks })
        : Promise.resolve(),
      v2call('run.finish', { run_id: runId, status, error_code: nativeStream.error ?? null }),
    ]).then(() => loadMessages(activeConversationId)).catch(error => {
      toast(classifyError(error).userMessage, 'error');
    });
  }, [activeConversationId, isStreaming, loadMessages, nativeStream, toast]);

  // The Tauri boundary intentionally owns the daemon socket. Until it exposes a
  // push bridge, replay new persisted events with a cursor so no event is lost.
  useEffect(() => {
    const runId = activeRunIdRef.current;
    if (!runId || !isStreaming) return;
    let cancelled = false;
    let cursor = eventsRef.current.at(-1)?.sequence ?? 0;

    const poll = async () => {
      try {
        const result = await v2call<RunEvent[]>('run.getEvents', {
          run_id: runId,
          after_sequence: cursor,
        });
        for (const event of result) {
          if (cancelled || event.sequence <= cursor) continue;
          cursor = event.sequence;
          handleStreamEvent({ ...event, runId });
        }
      } catch {
        // A transient daemon reconnect is recovered by the next replay poll.
      }
    };

    void poll();
    const timer = window.setInterval(poll, 300);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isStreaming, handleStreamEvent]);

  // ── Select conversation ──
  const handleSelectConversation = useCallback(async (id: string) => {
    setActiveConversationId(id);
    streamStateRef.current = null;
    setStreamState(null);
    setIsStreaming(false);
    setRunEvents([]);
    setArtifacts([]);
    eventsRef.current = [];
    latestRunIdRef.current = null;
    activeRunIdRef.current = null;
    await Promise.all([loadMessages(id), loadConversationRun(id)]);
  }, [loadConversationRun, loadMessages]);

  // ── Create conversation ──
  const handleCreateConversation = useCallback(async (projectPath = activeProjectPath) => {
    if (isCreatingConversation) return;
    setIsCreatingConversation(true);
    setInputDisabledReason('creating');

    try {
      // Find default provider/model
      const selection = selectAssistantModel(providers);
      if (!selection) {
        setInputDisabledReason(providerReadiness === 'no_model' ? 'no_model' : 'no_provider');
        return;
      }

      const result = await v2call<Conversation>('conversation.create', {
        mode: 'agent',
        title: t(locale, 'assistant.newConversation'),
        provider_id: selection.providerId,
        model_id: selection.modelId,
        project_id: projectPath,
        permission_profile_id: 'ask',
      });

      setConversations(prev => [{ ...result, project_id: projectPath }, ...prev]);
      setActiveConversationId(result.id);
      setMessages([]);
      setStreamState(null);
      streamStateRef.current = null;
      setIsStreaming(false);
      resetStream();
      setInputDisabledReason(null);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
      setInputDisabledReason(providerReadiness === 'ready' ? null : providerReadiness);
    } finally {
      setIsCreatingConversation(false);
    }
  }, [isCreatingConversation, providers, providerReadiness, activeProjectPath, locale, resetStream, toast]);

  const handleSelectModel = useCallback(async (providerId: string, modelId: string) => {
    if (!activeConversationId) return;
    try {
      const result = await v2call<{ updated_at?: string }>('conversation.update_model', {
        id: activeConversationId,
        provider_id: providerId,
        model_id: modelId,
      });
      setConversations(previous => previous.map(conversation =>
        conversation.id === activeConversationId
          ? {
              ...conversation,
              provider_id: providerId,
              model_id: modelId,
              updated_at: result.updated_at ?? conversation.updated_at,
            }
          : conversation,
      ));
    } catch (error) {
      toast(classifyError(error).userMessage, 'error');
    }
  }, [activeConversationId, toast]);

  const handlePermissionChange = useCallback(async (profile: AssistantPermissionProfile) => {
    if (!activeConversationId) return;
    try {
      await v2call('conversation.update_permission', { id: activeConversationId, permission_profile_id: profile });
      setConversations(current => current.map(conversation => conversation.id === activeConversationId
        ? { ...conversation, permission_profile_id: profile }
        : conversation));
    } catch (error) {
      toast(classifyError(error).userMessage, 'error');
    }
  }, [activeConversationId, toast]);

  // ── Archive conversation ──
  const handleArchiveConversation = useCallback(async (id: string) => {
    try {
      await v2call('conversation.archive', { id });
      setConversations(prev => prev.filter(c => c.id !== id));
      if (activeConversationId === id) {
        setActiveConversationId(null);
        setMessages([]);
        setStreamState(null);
        streamStateRef.current = null;
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [activeConversationId, toast]);

  // ── Delete conversation ──
  const handleDeleteConversation = useCallback(async (id: string) => {
    try {
      await v2call('conversation.delete', { id });
      setConversations(prev => prev.filter(c => c.id !== id));
      if (activeConversationId === id) {
        setActiveConversationId(null);
        setMessages([]);
        setStreamState(null);
        streamStateRef.current = null;
      }
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [activeConversationId, toast]);

  // ── Send message / Start run ──
  const handleSend = useCallback(async ({ content, attachments }: AssistantDraft): Promise<boolean> => {
    if (!activeConversationId) return false;

    const conversation = conversations.find(c => c.id === activeConversationId);
    if (!conversation) return false;

    const selection = selectAssistantModel(providers);
    const providerId = conversation.provider_id || selection?.providerId || '';
    const modelId = conversation.model_id || selection?.modelId || '';
    if (!providerId || !modelId) {
      setInputDisabledReason(providerReadiness === 'no_model' ? 'no_model' : 'no_provider');
      return false;
    }

    setIsStreaming(true);
    streamStateRef.current = null;
    setStreamState(null);
    setRunEvents([]);
    setArtifacts([]);
    setActiveRunStartedAt(null);
    setActiveRunFinishedAt(null);
    eventsRef.current = [];

    try {
      // Start a new run
      const run = await v2call<Run>('run.start', {
        conversation_id: activeConversationId,
        provider_id: providerId,
        model_id: modelId,
        content,
        attachments: attachments.map(file => ({ path: file.path, name: file.name, mime_type: file.mimeType, size: file.size })),
      });

      latestRunIdRef.current = run.id;
      activeRunIdRef.current = run.id;
      setActiveRunStartedAt(run.started_at ?? new Date().toISOString());
      setActiveRunFinishedAt(null);
      const newState = createAssistantStreamState(run.id);
      streamStateRef.current = newState;
      setStreamState(newState);
      persistedStreamRunRef.current = null;
      resetStream();

      // Reload messages to show the user message
      await loadMessages(activeConversationId);
      const assistantApi = window.nativesAPI?.assistant;
      if (!assistantApi) throw new Error('Assistant streaming API unavailable');
      await assistantApi.streamChat({
        sessionId: activeConversationId,
        model: modelId,
        messages: [{ role: 'user', content: [content, ...attachments.map(file => `[Attached file: ${file.path}]`)].filter(Boolean).join('\n') }],
      });
      return true;
    } catch (err) {
      setIsStreaming(false);
      toast(classifyError(err).userMessage, 'error');
      return false;
    }
  }, [activeConversationId, conversations, providers, providerReadiness, resetStream, loadMessages, toast]);

  // ── Stop / Cancel run ──
  const handleStop = useCallback(async () => {
    const runId = latestRunIdRef.current;
    if (!runId) return;
    const previousState = streamStateRef.current;
    try {
      setStreamState(previous => {
        if (!previous) return previous;
        const next = { ...previous, status: 'cancelling' as const };
        streamStateRef.current = next;
        return next;
      });
      await v2call('run.cancel', { run_id: runId });
      const assistantApi = window.nativesAPI?.assistant;
      if (assistantApi && activeConversationId) await assistantApi.cancelStream(activeConversationId);
      // Keep event replay active until the daemon publishes `interrupted`.
    } catch (error) {
      if (previousState) {
        streamStateRef.current = previousState;
        setStreamState(previousState);
      }
      toast(classifyError(error).userMessage, 'error');
    }
  }, [activeConversationId, toast]);

  // ── Retry run ──
  const handleRetry = useCallback(async () => {
    const runId = latestRunIdRef.current;
    if (!runId) return;
    const conversation = conversations.find(item => item.id === activeConversationId);
    if (!conversation?.provider_id || !conversation.model_id) return;
    const lastUserMessage = [...messages].reverse().find(message => message.role === 'user');
    if (!lastUserMessage) return;
    try {
      const run = await v2call<Run>('run.start', {
        conversation_id: activeConversationId,
        provider_id: conversation.provider_id,
        model_id: conversation.model_id,
        trigger_message_id: lastUserMessage.id,
      });
      latestRunIdRef.current = run.id;
      activeRunIdRef.current = run.id;
      setActiveRunStartedAt(run.started_at ?? new Date().toISOString());
      setActiveRunFinishedAt(null);
      setIsStreaming(true);
      eventsRef.current = [];
      setRunEvents([]);
      setArtifacts([]);
      const newState = createAssistantStreamState(run.id);
      streamStateRef.current = newState;
      setStreamState(newState);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [activeConversationId, conversations, messages, toast]);

  // ── Permission response ──
  const handlePermissionResponse = useCallback(async (requestId: string, approved: boolean, scope?: string) => {
    try {
      await v2call('permission.respond', {
        request_id: requestId,
        approved,
        scope: scope ?? 'once',
      });
      setRespondedPermissionId(requestId);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    }
  }, [toast]);

  // ── Update conversation title ──
  const handleUpdateTitle = useCallback(async (id: string, title: string) => {
    try {
      await v2call('conversation.rename', { id, title });
      setConversations(prev => prev.map(c =>
        c.id === id ? { ...c, title } : c
      ));
    } catch {
      // Silently fail
    }
  }, []);

  // ── Refresh ──
  const handleRefresh = useCallback(async () => {
    const connected = await checkDaemonStatus();
    if (!connected) return;
    await loadConversations();
    await loadProviders();
    if (activeConversationId) {
      await loadMessages(activeConversationId);
    }
  }, [activeConversationId, checkDaemonStatus, loadMessages]);

  const handleSelectProject = useCallback((projectPath: string | null) => {
    setActiveProjectPath(projectPath);
    if (projectPath) {
      void writeActiveProject(window.nativesAPI, projectPath).catch(error => {
        toast(classifyError(error).userMessage, 'error');
      });
    } else {
      // Clear active project selection (unclassified context)
      void writeActiveProject(window.nativesAPI, null).catch(error => {
        toast(classifyError(error).userMessage, 'error');
      });
    }
  }, [toast]);

  const handlePickProject = useCallback(async () => {
    try {
      const projectPath = await window.nativesAPI?.dialog?.pickDirectory();
      // User cancelled directory selection — no error
      if (!projectPath) return;

      // Register project with daemon
      const result = await window.nativesAPI?.project.register(projectPath);
      if (!result) throw new Error('Project API unavailable');
      handleSelectProject(result.path);
      await Promise.all([loadConversations(), loadProjects()]);
    } catch (error) {
      const classified = classifyError(error);
      // Do not show toast for cancellation — it's not an error
      if (classified.category === 'PROJECT_PATH_REQUIRED') return;
      toast(classified.userMessage, 'error');
    }
  }, [handleSelectProject, loadConversations, loadProjects, toast]);

  // ── Determine active conversation ──
  const activeConversation = conversations.find(c => c.id === activeConversationId);
  const projectGroups = useMemo(() => {
    const groups = groupAssistantConversations(
      conversations.map(conversation => ({
        id: conversation.id,
        title: conversation.title,
        mode: conversation.mode,
        projectId: conversation.project_id ?? '',
        updatedAt: conversation.updated_at,
      })),
      registeredProjects.map(project => project.path),
      t(locale, 'assistant.unassignedProject'),
    );
    return groups;
  }, [conversations, locale, registeredProjects]);
  const creationState = projectCreationState({
    engine: daemonConnected ? 'ready' : (daemonError ? 'unavailable' : 'connecting'),
    providerReadiness,
  });
  const surfaceState = classifyAssistantSurface({
    bridge: !rendererOnly,
    engine: daemonConnected ? 'ready' : (daemonError ? 'failed' : 'connecting'),
    provider: providerReadiness,
  });
  const assistantEvents = useMemo<AssistantRunEvent[]>(() => {
    const runId = streamState?.runId ?? '';
    return runEvents.map(event => ({
      runId,
      sequence: event.sequence,
      timestamp: event.timestamp,
      type: event.type as AssistantRunEvent['type'],
      payload: (event.payload ?? {}) as Record<string, unknown>,
    }));
  }, [streamState?.runId, runEvents]);
  const fileChanges = useMemo(() => streamState?.fileChanges ?? [], [streamState?.fileChanges]);
  const workspaceActions = useMemo<AssistantWorkspaceActions>(() => ({
    selectConversation: id => { void handleSelectConversation(id); },
    selectProject: handleSelectProject,
    addProjectFolder: () => { void handlePickProject(); },
    createConversation: () => { void handleCreateConversation(); },
    createConversationInProject: path => { handleSelectProject(path); void handleCreateConversation(path); },
    renameConversation: (id, title) => { void handleUpdateTitle(id, title); },
    archiveConversation: id => { void handleArchiveConversation(id); },
    deleteConversation: id => { void handleDeleteConversation(id); },
    retryRun: () => { void handleRetry(); },
    respondPermission: (requestId, approved) => { void handlePermissionResponse(requestId, approved); },
  }), [handleArchiveConversation, handleCreateConversation, handleDeleteConversation, handlePermissionResponse, handlePickProject, handleRetry, handleSelectConversation, handleSelectProject, handleUpdateTitle]);

  useEffect(() => {
    registerActions(workspaceActions);
    return () => registerActions(null);
  }, [registerActions, workspaceActions]);

  useEffect(() => {
    publishNavigation({
      groups: projectGroups,
      selectedId: activeConversationId,
      activeProjectPath,
      loading: loadingConversations,
      creationState,
      isCreatingConversation,
    });
  }, [activeConversationId, activeProjectPath, isCreatingConversation, creationState, loadingConversations, projectGroups, publishNavigation]);

  useEffect(() => {
    publishRuntime({
      conversationId: activeConversationId,
      conversationTitle: activeConversation?.title ?? null,
      conversationMode: activeConversation?.mode ?? 'chat',
      providerId: activeConversation?.provider_id ?? '',
      modelId: activeConversation?.model_id ?? '',
      runId: streamState?.runId ?? null,
      runStatus: streamState?.status ?? 'idle',
      runStartedAt: activeRunStartedAt,
      runFinishedAt: activeRunFinishedAt,
      events: assistantEvents,
      fileChanges,
      artifacts,
      usage: streamState?.usage ?? { inputTokens: null, outputTokens: null, reasoningTokens: null },
    });
  }, [activeConversation, activeConversationId, activeRunFinishedAt, activeRunStartedAt, streamState, artifacts, assistantEvents, fileChanges, publishRuntime]);

  // ── Map messages to timeline format ──
  const timelineMessages = messages.map(mapMessageToTimeline);
  const modelSelectorProviders: ProviderWithModels[] = providers.map(provider => ({
    id: provider.id,
    name: provider.display_name,
    presetName: provider.provider_type,
    baseUrl: provider.api_base_url,
    keys: [],
    models: provider.models?.map(model => ({ id: model.id, displayName: model.display_name })),
  }));

  // Add streaming blocks as a pending message if streaming
  const streamingBlocks = streamState?.blocks ?? [];
  const activeRunStatus = streamState?.status ?? 'idle';
  const timelineWithStreaming = streamingBlocks.length > 0 && activeRunStatus !== 'completed'
    ? [
        ...timelineMessages,
        {
          id: 'streaming',
          role: 'assistant' as const,
          contentBlocks: streamingBlocks,
          status: isStreaming ? 'streaming' : activeRunStatus,
          createdAt: activeRunStartedAt ?? new Date().toISOString(),
          startedAt: activeRunStartedAt ?? undefined,
          finishedAt: activeRunFinishedAt ?? undefined,
        },
      ]
    : timelineMessages;

  // ── Render: daemon offline ──
  if (!daemonConnected && !daemonError) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="flex flex-col items-center gap-3 text-[var(--text-disabled)]">
          <Loader2 size={24} className="animate-spin" />
          <span className="text-sm">{t(locale, 'assistant.connecting')}</span>
        </div>
      </div>
    );
  }

  if (daemonError) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="flex flex-col items-center gap-3 max-w-sm text-center">
          <WifiOff size={32} className="text-[var(--text-disabled)]" />
          <div className="text-sm font-medium text-[var(--text-secondary)]">
            {surfaceState === 'renderer_only'
              ? t(locale, 'assistant.desktopEngineRequiredTitle')
              : t(locale, 'assistant.assistantUnavailable')}
          </div>
          <div className="text-xs text-[var(--text-disabled)]">{daemonError}</div>
          <button
            onClick={handleRefresh}
            className="px-4 py-1.5 rounded-lg bg-[var(--primary-soft)] text-[var(--primary)] text-sm font-medium hover:opacity-80 transition-opacity"
          >
            {surfaceState === 'renderer_only' ? t(locale, 'assistant.openDesktopApp') : t(locale, 'assistant.retry')}
          </button>
        </div>
      </div>
    );
  }

  // ── Render: main layout ──
  return (
    <div className="flex h-full w-full" style={{ fontFamily: 'inherit' }}>
      {/* ── Center Panel: Conversation Timeline ── */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Header */}
        {activeConversation && (
          <div className="shrink-0 border-b border-[var(--border-subtle)] px-4 py-2 flex items-center gap-3">
            <div className="flex-1 min-w-0">
              <input
                type="text"
                className="w-full bg-transparent text-sm font-medium text-[var(--text-primary)] border-none outline-none placeholder-[var(--text-disabled)]"
                value={activeConversation.title}
                placeholder={t(locale, 'assistant.conversationTitle')}
                onChange={(e) => {
                  setConversations(prev => prev.map(c =>
                    c.id === activeConversationId ? { ...c, title: e.target.value } : c
                  ));
                }}
                onBlur={(e) => {
                  const title = e.target.value.trim();
                  if (title && activeConversationId) {
                    handleUpdateTitle(activeConversationId, title);
                  }
                }}
              />
            </div>
          </div>
        )}

        {/* Empty state when no conversation selected */}
        {!activeConversationId && (
          <div className="flex-1 flex items-center justify-center">
            <div className="text-center">
              <div className="text-sm text-[var(--text-disabled)] mb-2">
                {t(locale, 'assistant.selectConversation')}
              </div>
              {creationState !== 'ready' && (
                <div className="mt-3 text-xs text-[var(--text-disabled)]">
                  {providerLoadError ?? (providerReadiness === 'no_model'
                  ? t(locale, 'assistant.noModelHint')
                  : t(locale, 'assistant.configureProvider'))}
                </div>
              )}
            </div>
          </div>
        )}

        {/* Timeline */}
        {activeConversationId && (
          <>
            <div className="flex-1 overflow-y-auto">
              <ConversationTimeline
                messages={timelineWithStreaming}
                loading={loadingMessages}
                locale={locale}
                onRetry={handleRetry}
              />
            </div>

            {nativeStream.permissionRequest && nativeStream.permissionRequest.id !== respondedPermissionId && (
              <div className="mx-auto w-full max-w-[860px] px-5">
                <PermissionRequestCard
                  request={{ ...nativeStream.permissionRequest, status: 'pending', createdAt: new Date().toISOString() }}
                  onApprove={(id, scope) => { void handlePermissionResponse(id, true, scope); }}
                  onReject={id => { void handlePermissionResponse(id, false); }}
                  locale={locale}
                />
              </div>
            )}

            {/* Input area */}
            <div className="shrink-0">
              <MessageInput
                locale={locale}
                onSend={handleSend}
                onStop={handleStop}
                isStreaming={isStreaming}
                disabled={!activeConversationId}
                inputDisabledReason={inputDisabledReason}
                permissionProfile={normalizePermissionProfile(activeConversation?.permission_profile_id)}
                onPermissionChange={handlePermissionChange}
                providers={modelSelectorProviders}
                selectedProviderId={activeConversation?.provider_id ?? ''}
                selectedModel={activeConversation?.model_id}
                onSelectModel={handleSelectModel}
              />
            </div>
          </>
        )}
      </div>

      {rightPanelOpen ? (
        <aside className="flex w-80 shrink-0 flex-col border-l border-[var(--border-subtle)] bg-[var(--surface)]">
          <div className="flex h-11 items-center justify-between border-b border-[var(--border-subtle)] px-3">
            <span className="text-sm font-medium text-[var(--text-secondary)]">{activeConversation?.model_id || t(locale, 'assistant.selectConversation')}</span>
            <button type="button" onClick={() => setRightPanelOpen(false)} className="rounded p-1 text-[var(--text-tertiary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]" aria-label={t(locale, 'common.collapse')} title={t(locale, 'common.collapse')}><PanelRightClose size={15} /></button>
          </div>
          <div className="border-b border-[var(--border-subtle)] px-3 py-2 text-xs text-[var(--text-disabled)]">
            <div className="truncate">{activeConversation?.provider_id || '—'}</div>
            <div className="mt-1">{streamState?.usage.inputTokens ?? 0} / {streamState?.usage.outputTokens ?? 0} tokens</div>
          </div>
          <RunInspector
            runId={streamState?.runId ?? null}
            status={streamState?.status ?? 'idle'}
            events={runEvents.map(event => ({ sequence: event.sequence, type: event.type, timestamp: event.timestamp }))}
            artifacts={artifacts}
            subAgents={[]}
            locale={locale}
          />
        </aside>
      ) : (
        <div className="w-9 shrink-0 border-l border-[var(--border-subtle)] bg-[var(--surface)] pt-2">
          <button type="button" onClick={() => setRightPanelOpen(true)} className="m-1 rounded p-1 text-[var(--text-tertiary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]" aria-label={t(locale, 'common.expand')} title={t(locale, 'common.expand')}><PanelRightOpen size={15} /></button>
        </div>
      )}

    </div>
  );
}
