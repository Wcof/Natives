import { getDb } from './database';
import { checkPermission } from './bridge-host';

// ── Plugin IPC Bridge ──
// Messages are relayed through the main process (or bridge host)
// due to sandbox iframe restrictions (no BroadcastChannel/SharedWorker)

interface IpcMessage {
  from: string;
  to: string | '*'; // '*' for broadcast
  channel: string;
  payload: unknown;
  timestamp: number;
}

type IpcListener = (message: IpcMessage) => void;

const listeners = new Map<string, Set<IpcListener>>(); // moduleId → listeners
const broadcastListeners = new Set<IpcListener>(); // global for '*'

// ── Send ──

export function sendMessage(from: string, targetId: string, channel: string, payload: unknown): void {
  // Permission check
  if (!checkPermission(from, 'ipc:send')) {
    throw new Error(`Module '${from}' does not have 'ipc:send' permission`);
  }
  if (!checkPermission(targetId, 'ipc:receive')) {
    throw new Error(`Module '${targetId}' does not have 'ipc:receive' permission`);
  }

  const message: IpcMessage = {
    from,
    to: targetId,
    channel,
    payload,
    timestamp: Date.now(),
  };

  // Deliver to target
  const targetListeners = listeners.get(targetId);
  if (targetListeners) {
    for (const listener of targetListeners) {
      listener(message);
    }
  }
}

// ── Broadcast ──

export function broadcastMessage(from: string, channel: string, payload: unknown): void {
  if (!checkPermission(from, 'ipc:send')) {
    throw new Error(`Module '${from}' does not have 'ipc:send' permission`);
  }

  const message: IpcMessage = {
    from,
    to: '*',
    channel,
    payload,
    timestamp: Date.now(),
  };

  // Deliver to all listeners
  for (const listener of broadcastListeners) {
    listener(message);
  }
  // Also deliver to module-specific listeners
  for (const [, moduleListeners] of listeners) {
    for (const listener of moduleListeners) {
      listener(message);
    }
  }
}

// ── Subscribe ──

export function onMessage(moduleId: string, listener: IpcListener): () => void {
  if (!listeners.has(moduleId)) {
    listeners.set(moduleId, new Set());
  }
  listeners.get(moduleId)!.add(listener);

  return () => {
    listeners.get(moduleId)?.delete(listener);
  };
}

export function onBroadcast(listener: IpcListener): () => void {
  broadcastListeners.add(listener);
  return () => {
    broadcastListeners.delete(listener);
  };
}

// ── Message Logging (audit) ──

const messageLog: IpcMessage[] = [];
const MAX_LOG = 1000;

export function logMessage(msg: IpcMessage): void {
  messageLog.push(msg);
  if (messageLog.length > MAX_LOG) {
    messageLog.shift();
  }
}

export function getMessageLog(): IpcMessage[] {
  return [...messageLog];
}

export function clearMessageLog(): void {
  messageLog.length = 0;
}
