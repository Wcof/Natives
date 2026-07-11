// ─── Assistant Daemon Client ─────────────────────────────
//
// Typed client for communicating with the Rust Agent Daemon via
// Tauri invoke calls. All daemon interactions go through this client
// and are exposed via window.nativesAPI.assistantV2.

import type {
  DaemonClientConfig,
  DaemonClientEvent,
  ConnectionState,
  ConnectionStatus,
  RpcRequest,
  RpcResponse,
  DaemonError,
  HandshakeResponse,
  DaemonStatus,
  StreamEvent,
} from './types';

type EventCallback = (event: DaemonClientEvent) => void;

/**
 * Typed client for the Natives Agent Daemon.
 *
 * Communicates with the Rust daemon through Tauri invoke commands.
 * Handles connection lifecycle, request correlation, and reconnection
 * with replay from last acknowledged sequence.
 */
export class DaemonClient {
  private config: DaemonClientConfig;
  private sessionToken: string | null = null;
  private state: ConnectionState = 'disconnected';
  private reconnectAttempts = 0;
  private lastAcknowledgedSequence = 0;
  private pendingRequests = new Map<string, {
    resolve: (value: unknown) => void;
    reject: (reason: Error) => void;
    timeout: ReturnType<typeof setTimeout>;
  }>();
  private eventCallbacks: EventCallback[] = [];
  private requestCounter = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private streamSubscriptionId: string | null = null;
  private invokeFn: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

  constructor(config: DaemonClientConfig, invokeFn?: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>) {
    this.config = {
      reconnectDelayMs: 1000,
      maxReconnectAttempts: 10,
      ...config,
    };
    // Use provided invoke function or import the real one
    this.invokeFn = invokeFn || this.defaultInvoke.bind(this);
  }

  private async defaultInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke(cmd, args);
  }

  // ─── Connection Management ─────────────────────────────

  /**
   * Connect to the daemon: perform handshake to exchange bootstrap token
   * for a session token.
   */
  async connect(): Promise<void> {
    this.setState('connecting');

    try {
      // Perform handshake via Tauri invoke
      const handshakeResponse = await this.invokeFn('daemon_handshake', {
        bootstrapToken: this.config.bootstrapToken,
        clientVersion: this.config.protocolVersion,
        clientId: this.config.clientId,
      }) as HandshakeResponse;

      if (!handshakeResponse.accepted) {
        throw new Error(`Handshake rejected: ${handshakeResponse.upgradeRequired || 'unknown reason'}`);
      }

      this.sessionToken = handshakeResponse.sessionToken;
      this.reconnectAttempts = 0;
      this.setState('connected');
      this.emit({ type: 'connected' });
    } catch (error) {
      this.setState('error');
      this.emit({ type: 'error', error: String(error) });
      throw error;
    }
  }

  /**
   * Disconnect from the daemon.
   */
  disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.sessionToken = null;
    this.setState('disconnected');
    this.emit({ type: 'disconnected' });
  }

  /**
   * Attempt to reconnect with exponential backoff.
   */
  private async attemptReconnect(): Promise<void> {
    if (this.reconnectAttempts >= (this.config.maxReconnectAttempts ?? 10)) {
      this.setState('error');
      this.emit({
        type: 'error',
        error: `Max reconnect attempts (${this.config.maxReconnectAttempts}) reached`,
      });
      return;
    }

    this.reconnectAttempts++;
    this.setState('reconnecting');
    this.emit({ type: 'reconnecting', attempt: this.reconnectAttempts });

    const delay = Math.min(
      1000 * Math.pow(2, this.reconnectAttempts - 1),
      30000
    );

    this.reconnectTimer = setTimeout(async () => {
      try {
        await this.connect();

        // Replay events from last acknowledged sequence
        if (this.lastAcknowledgedSequence > 0) {
          this.subscribeToStream();
        }
      } catch {
        // Reconnect failed, try again
        this.attemptReconnect();
      }
    }, delay);
  }

  // ─── RPC Calls ─────────────────────────────────────────

  /**
   * Send an RPC request and wait for the response.
   */
  async call<T = unknown>(method: string, params: unknown = {}): Promise<T> {
    if (!this.sessionToken) {
      throw new Error('Not connected to daemon');
    }

    const requestId = `req-${++this.requestCounter}-${Date.now()}`;
    const timeout = 30000; // 30s timeout

    // Create the request
    const request: RpcRequest = {
      protocolVersion: this.config.protocolVersion,
      requestId,
      clientId: this.config.clientId,
      sessionToken: this.sessionToken,
      method,
      params,
    };

    // Send via Tauri invoke
    return new Promise<T>((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        this.pendingRequests.delete(requestId);
        reject(new Error(`RPC call "${method}" timed out after ${timeout}ms`));
      }, timeout);

      this.pendingRequests.set(requestId, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timeout: timeoutId,
      });

      this.invokeFn('daemon_rpc_call', { request })
        .then((response) => {
          clearTimeout(timeoutId);
          this.pendingRequests.delete(requestId);
          resolve(response as T);
        })
        .catch((error) => {
          clearTimeout(timeoutId);
          this.pendingRequests.delete(requestId);
          reject(error instanceof Error ? error : new Error(String(error)));
        });
    });
  }

  /**
   * Get daemon status.
   */
  async getStatus(): Promise<DaemonStatus> {
    return this.call<DaemonStatus>('daemon.getStatus');
  }

  /**
   * Ping the daemon to check connectivity.
   */
  async ping(): Promise<boolean> {
    try {
      const result = await this.call<{ pong: boolean }>('daemon.ping');
      return result.pong;
    } catch {
      return false;
    }
  }

  // ─── Stream Subscription ───────────────────────────────

  /**
   * Subscribe to run events via Tauri event listener.
   */
  private async subscribeToStream(): Promise<void> {
    if (this.streamSubscriptionId) {
      return; // Already subscribed
    }

    this.streamSubscriptionId = 'daemon:stream_event';
    const { listen } = await import('@tauri-apps/api/event');

    await listen<StreamEvent>(this.streamSubscriptionId, (event) => {
      const streamEvent = event.payload;
      this.emit({ type: 'stream_event', event: streamEvent });

      // Update last acknowledged sequence
      if (streamEvent.sequence > this.lastAcknowledgedSequence) {
        this.lastAcknowledgedSequence = streamEvent.sequence;
      }
    });
  }

  // ─── Event Handling ────────────────────────────────────

  /**
   * Register a callback for daemon client events.
   */
  onEvent(callback: EventCallback): () => void {
    this.eventCallbacks.push(callback);
    return () => {
      this.eventCallbacks = this.eventCallbacks.filter(cb => cb !== callback);
    };
  }

  private emit(event: DaemonClientEvent): void {
    this.eventCallbacks.forEach(cb => {
      try {
        cb(event);
      } catch {
        // Swallow callback errors
      }
    });
  }

  // ─── State Management ──────────────────────────────────

  private setState(state: ConnectionState): void {
    this.state = state;
  }

  getState(): ConnectionState {
    return this.state;
  }

  getConnectionStatus(): ConnectionStatus {
    return {
      state: this.state,
      reconnectAttempts: this.reconnectAttempts,
      lastAcknowledgedSequence: this.lastAcknowledgedSequence,
    };
  }

  getSessionToken(): string | null {
    return this.sessionToken;
  }
}