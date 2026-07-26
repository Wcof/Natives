/**
 * Capability gate — honest UI surface from daemon.getCapabilities().methods.
 *
 * Rule: never render / call an RPC that is not advertised. `methods` is
 * `IMPLEMENTED_METHODS ∪ HOST_IMPLEMENTED_METHODS` from
 * `crates/assistant-protocol/src/v2/methods.rs`; anything absent from it must stay
 * hidden. Catalogued-but-deliberately-unimplemented methods — `mcp.call` (closed
 * transport bypass) and `mcp.auth.oauthStart` / `mcp.auth.oauthCallback` (no browser
 * or redirect listener exists) — are never advertised, so the gate keeps them hidden
 * on its own; do not special-case them here.
 *
 * The gate is only as honest as the advertisement. `IMPLEMENTED_METHODS` had drifted
 * ahead of the daemon's real dispatch arms, which opened this gate for methods that
 * then answered `internal_error`; that invariant is now pinned by
 * `src-agent-daemon/tests/rpc_dispatch_contract.rs`.
 */
import type { ConnectionState, DaemonCapabilities } from '@/lib/assistant-protocol';

/**
 * Methods the GUI may call once advertised (contract spot-check).
 *
 * Historically these were all host-owned. After the daemon cutover only
 * `artifact.reveal` is still host-only — it needs the desktop file manager — and the
 * rest are served by the daemon. The list is kept as the UI's called-method inventory;
 * membership here grants nothing, `hasMethod` against the live advertisement does.
 */
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
  'subagent.list',
  'subagent.touch',
  'subagent.switchRoute',
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
  return hasMethod(caps, 'workspace.restore') && hasMethod(caps, 'workspace.restorePreview');
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
