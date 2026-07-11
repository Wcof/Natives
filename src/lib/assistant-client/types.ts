// ─── Assistant Daemon Client Types ───────────────────────
//
// Typed request/response types for the assistant daemon RPC.

// ─── RPC Envelope ────────────────────────────────────────

export interface RpcRequest {
  protocolVersion: string;
  requestId: string;
  clientId: string;
  sessionToken: string;
  method: string;
  params: unknown;
}

export interface RpcResponse {
  protocolVersion: string;
  requestId: string;
  success: boolean;
  data?: unknown;
  error?: DaemonError;
}

export interface DaemonError {
  code: string;
  category: ErrorCategory;
  retryable: boolean;
  userMessageKey: string;
  technicalMessage: string;
  recoveryActions: string[];
  correlationId: string;
}

export type ErrorCategory =
  | 'validation' | 'auth' | 'not_found' | 'conflict' | 'rate_limited'
  | 'timeout' | 'internal' | 'provider' | 'network' | 'permission_denied'
  | 'unsupported' | 'extension';

// ─── Handshake ───────────────────────────────────────────

export interface HandshakeRequest {
  clientVersion: string;
  clientId: string;
  bootstrapToken: string;
}

export interface HandshakeResponse {
  sessionToken: string;
  daemonVersion: string;
  protocolVersion: string;
  accepted: boolean;
  upgradeRequired?: string;
}

// ─── Daemon Status ───────────────────────────────────────

export interface DaemonStatus {
  version: string;
  protocolVersion: string;
  uptimeSecs: number;
  pid: number;
  activeRuns: number;
  activeExtensions: number;
  providerCount: number;
  memoryUsageMb: number;
  health: DaemonHealth;
}

export type DaemonHealth =
  | { type: 'healthy' }
  | { type: 'degraded'; reasons: string[] }
  | { type: 'unhealthy'; message: string };

// ─── Stream Event ────────────────────────────────────────

export interface StreamEvent {
  runId: string;
  sequence: number;
  type: string;
  payload: unknown;
}

// ─── Client Configuration ────────────────────────────────

export interface DaemonClientConfig {
  /** Socket path (Unix) or pipe name (Windows) */
  socketPath: string;
  /** Bootstrap token for initial handshake */
  bootstrapToken: string;
  /** Client identifier */
  clientId: string;
  /** Protocol version */
  protocolVersion: string;
  /** Reconnect delay in ms (default: 1000) */
  reconnectDelayMs?: number;
  /** Max reconnect attempts (default: 10) */
  maxReconnectAttempts?: number;
}

// ─── Connection State ────────────────────────────────────

export type ConnectionState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'error';

export interface ConnectionStatus {
  state: ConnectionState;
  lastError?: string;
  reconnectAttempts: number;
  lastAcknowledgedSequence: number;
}

// ─── Event types ─────────────────────────────────────────

export type DaemonClientEvent =
  | { type: 'connected' }
  | { type: 'disconnected' }
  | { type: 'reconnecting'; attempt: number }
  | { type: 'error'; error: string }
  | { type: 'stream_event'; event: StreamEvent }
  | { type: 'status_change'; status: ConnectionStatus };