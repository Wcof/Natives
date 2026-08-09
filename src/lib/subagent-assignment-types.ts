/**
 * Subagent assignment modal shared types (W4): moved out of the component so
 * hooks never depend on component internals. The component re-exports them.
 */
import type { SubagentAssignmentMode, SubagentRouteBinding } from './assistant-protocol';

/** One selectable key for a subagent assignment. */
export interface AssignmentKeyOption {
  providerId: string;
  providerName: string;
  keyId: string;
  keyLabel: string;
  modelId: string;
  models: Array<{ id: string; displayName?: string }>;
  isActive?: boolean;
  status?: 'untested' | 'valid' | 'invalid' | 'rate_limited' | 'unavailable' | string;
}

/** Payload confirming a subagent assignment (bound key / model). */
export interface SubagentAssignmentConfirmPayload {
  mode: SubagentAssignmentMode;
  /** Per-task assignments (call_id → binding). Always filled for default/custom. */
  assignments: Array<{
    callId: string;
    providerId: string;
    keyId: string;
    modelId: string;
  }>;
  /** Random-mode pool of valid keys (empty for other modes). */
  pool: Array<{ providerId: string; keyId: string; modelId: string }>;
  /** Flat bindings for legacy daemon path (policy upsert). */
  bindings: SubagentRouteBinding[];
  sessionId?: string | null;
}
