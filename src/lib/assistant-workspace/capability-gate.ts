/**
 * Capability gate — honest UI surface from daemon.getCapabilities().methods.
 *
 * Rule: never render / call an RPC that is not advertised. Known-but-unimplemented
 * methods (run.rewind, conversation.getContextUsage, task.list, …) must stay hidden
 * until IMPLEMENTED ∪ HOST advertises them.
 */
import type { ConnectionState, DaemonCapabilities } from '@/lib/assistant-protocol';

/** Host-side methods the GUI may call when advertised (contract spot-check). */
export const HOST_METHODS_UI = [
  'promptQueue.enqueue',
  'promptQueue.list',
  'promptQueue.update',
  'promptQueue.remove',
  'promptQueue.reorder',
  'promptQueue.sendNow',
  'permission.respond',
  'permission.listPending',
  'interaction.respond',
  'interaction.listPending',
  'artifact.list',
  'artifact.open',
  'artifact.reveal',
  'run.listChildren',
] as const;

export type CapabilityMethodName = string;

export function hasMethod(
  caps: DaemonCapabilities | null | undefined,
  name: CapabilityMethodName,
): boolean {
  if (!caps?.methods?.length) return false;
  return caps.methods.includes(name);
}

export function canRewind(caps: DaemonCapabilities | null | undefined): boolean {
  return hasMethod(caps, 'run.rewind') || hasMethod(caps, 'run.rewindPreview');
}

export function canShowContextUsage(caps: DaemonCapabilities | null | undefined): boolean {
  return hasMethod(caps, 'conversation.getContextUsage');
}

export function canListTasks(caps: DaemonCapabilities | null | undefined): boolean {
  // Prefer task.list; child-run tree via run.listChildren is a weaker fallback.
  return hasMethod(caps, 'task.list') || hasMethod(caps, 'run.listChildren');
}

export function canListTaskDepth(caps: DaemonCapabilities | null | undefined): boolean {
  return hasMethod(caps, 'task.list');
}

export function canCancelTask(caps: DaemonCapabilities | null | undefined): boolean {
  return hasMethod(caps, 'task.cancel');
}

export function canInterject(caps: DaemonCapabilities | null | undefined): boolean {
  return hasMethod(caps, 'promptQueue.interject');
}

/**
 * Blocking engine recovery: do not pretend chat/execution is available.
 * - fatal / incompatible always block
 * - offline without capabilities = daemon unavailable after a failed connect
 * Initial `disconnected` (before boot) is not recovery — workbench is still connecting.
 */
export function needsEngineRecovery(
  connection: ConnectionState,
  caps: DaemonCapabilities | null | undefined,
): boolean {
  if (connection === 'fatal' || connection === 'incompatible') return true;
  if (connection === 'offline' && !caps) return true;
  return false;
}

export function buildDiagnosticsText(input: {
  connection: ConnectionState;
  connectionError?: string | null;
  protocolVersion?: string | null;
  methodsCount?: number;
  clientVersion?: string;
  daemonVersion?: string;
  reconnectAttempts?: number;
}): string {
  const lines = [
    `connection=${input.connection}`,
    `error=${input.connectionError ?? ''}`,
    `protocol=${input.protocolVersion ?? ''}`,
    `methods=${input.methodsCount ?? 0}`,
    `reconnectAttempts=${input.reconnectAttempts ?? 0}`,
  ];
  if (input.clientVersion) lines.push(`clientVersion=${input.clientVersion}`);
  if (input.daemonVersion) lines.push(`daemonVersion=${input.daemonVersion}`);
  lines.push(`ts=${new Date().toISOString()}`);
  return lines.join('\n');
}
