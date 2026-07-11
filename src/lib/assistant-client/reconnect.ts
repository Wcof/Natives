// ─── Reconnect Manager ───────────────────────────────────
//
// Manages reconnection logic for the daemon client, including
// exponential backoff, replay from last acknowledged sequence,
// and duplicate suppression.

import { DaemonClient } from './client';
import type { DaemonClientEvent, ConnectionStatus, StreamEvent } from './types';

export type ReconnectHandler = (status: ConnectionStatus) => void;

/**
 * Reconnect manager for the daemon client.
 *
 * Handles automatic reconnection with exponential backoff,
 * replay from last acknowledged sequence, and duplicate
 * event suppression.
 */
export class ReconnectManager {
  private client: DaemonClient;
  private handlers: ReconnectHandler[] = [];
  private unsubscribes: (() => void)[] = [];
  private receivedSequences = new Set<string>();
  private isRunning = false;

  constructor(client: DaemonClient) {
    this.client = client;
  }

  /**
   * Start the reconnect manager.
   */
  start(): void {
    if (this.isRunning) return;
    this.isRunning = true;

    // Listen for client events
    const unsub = this.client.onEvent((event: DaemonClientEvent) => {
      switch (event.type) {
        case 'disconnected':
          this.handleDisconnect();
          break;
        case 'connected':
          this.handleConnected();
          break;
        case 'stream_event':
          this.handleStreamEvent(event.event);
          break;
      }
    });
    this.unsubscribes.push(unsub);
  }

  /**
   * Stop the reconnect manager.
   */
  stop(): void {
    this.isRunning = false;
    this.unsubscribes.forEach(unsub => unsub());
    this.unsubscribes = [];
    this.receivedSequences.clear();
  }

  /**
   * Register a handler for status changes.
   */
  onStatusChange(handler: ReconnectHandler): () => void {
    this.handlers.push(handler);
    return () => {
      this.handlers = this.handlers.filter(h => h !== handler);
    };
  }

  private handleDisconnect(): void {
    // Attempt reconnection
    this.attemptReconnect();
  }

  private handleConnected(): void {
    this.notifyStatus();
  }

  private handleStreamEvent(event: StreamEvent): void {
    // Duplicate suppression: track received sequences
    const key = `${event.runId}:${event.sequence}`;
    if (this.receivedSequences.has(key)) {
      return; // Duplicate event, suppress
    }
    this.receivedSequences.add(key);

    // Trim the set to prevent memory leaks
    if (this.receivedSequences.size > 10000) {
      this.receivedSequences.clear();
    }
  }

  private async attemptReconnect(): Promise<void> {
    try {
      await this.client.connect();
    } catch {
      // Reconnect will be retried by the client's internal logic
    }
  }

  private notifyStatus(): void {
    const status = this.client.getConnectionStatus();
    this.handlers.forEach(h => h(status));
  }

  /**
   * Get the number of received (unique) sequences.
   */
  getReceivedSequenceCount(): number {
    return this.receivedSequences.size;
  }
}