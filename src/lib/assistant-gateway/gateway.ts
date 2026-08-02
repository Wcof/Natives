/**
 * AssistantGateway — the only seam between GUI and Engine.
 * Components must not call window.nativesAPI.assistantV2 / streamChat / Tauri invoke.
 */
import type {
  AssistantMethod,
  ConversationSnapshot,
  DaemonCapabilities,
  RunEvent,
} from '@/lib/assistant-protocol';
import type { ProjectionRecovery } from '@/lib/assistant-protocol';

export interface AssistantGateway {
  connect(): Promise<void>;
  disconnect(): Promise<void>;
  request<T>(method: AssistantMethod, params?: unknown): Promise<T>;
  subscribe(runId: string, afterSequence: number): AsyncIterable<RunEvent>;
  getSnapshot(conversationId: string): Promise<ConversationSnapshot>;
  /** Optional: listen to connection-level status from the adapter. */
  getCapabilities?(): Promise<DaemonCapabilities | null>;
  /** Renderer-local projection status; never a synthetic daemon event. */
  getProjectionRecovery?(runId: string): ProjectionRecovery | undefined;
}

export type GatewayFactory = () => AssistantGateway;
