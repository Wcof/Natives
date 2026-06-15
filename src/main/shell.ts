// ── Shell Manager ──
// PTY terminal via node-pty with graceful degradation to child_process.spawn

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let ptySpawn: any = null;
let usePty = true;

try {
  // Dynamic require for optional native module
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const pty = require('node-pty');
  ptySpawn = pty.spawn;
} catch {
  usePty = false;
}

// ── Session types ──

export interface ShellSession {
  id: string;
  pid: number;
  cols: number;
  rows: number;
  createdAt: Date;
  env: Record<string, string>;
}

type DataHandler = (data: string) => void;
type ExitHandler = (exitCode: number, signal?: number) => void;

const sessions = new Map<string, ShellSession>();
const dataListeners = new Map<string, Set<DataHandler>>();
const exitListeners = new Map<string, Set<ExitHandler>>();
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const processMap = new Map<string, any>(); // raw process reference (IPty or ChildProcess)
const writeHandlers = new Map<string, (data: string) => void>();
const killHandlers = new Map<string, () => void>();

let sessionCounter = 0;

// ── PTY Session ──

function createPTYSession(shell: string, env: Record<string, string>): ShellSession {
  const id = `session-${++sessionCounter}`;
  const cols = 80;
  const rows = 24;

  if (usePty && ptySpawn) {
    try {
      const ptyProcess = ptySpawn(shell, [], {
        name: 'xterm-256color',
        cols,
        rows,
        cwd: process.env.HOME,
        env: { ...process.env, ...env },
      });

      writeHandlers.set(id, (data: string) => ptyProcess.write(data));
      killHandlers.set(id, () => ptyProcess.kill());

      ptyProcess.onData((data: string) => {
        const handlers = dataListeners.get(id);
        if (handlers) handlers.forEach((h) => h(data));
      });

      ptyProcess.onExit(({ exitCode, signal }: { exitCode: number; signal?: number }) => {
        const handlers = exitListeners.get(id);
        if (handlers) handlers.forEach((h) => h(exitCode, signal));
        sessions.delete(id);
      });

      processMap.set(id, ptyProcess);
    } catch {
      // PTY spawn failed, fall through to child_process fallback
      usePty = false;
      return createPTYSession(shell, env);
    }
  } else {
    // Fallback to child_process.spawn
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const { spawn } = require('child_process');
    const child = spawn(shell, [], {
      env: { ...process.env, ...env },
      cwd: process.env.HOME,
    });

    writeHandlers.set(id, (data: string) => {
      if (child.stdin?.writable) child.stdin.write(data);
    });
    killHandlers.set(id, () => child.kill());

    if (child.stdout) {
      child.stdout.on('data', (data: Buffer) => {
        const handlers = dataListeners.get(id);
        if (handlers) handlers.forEach((h) => h(data.toString()));
      });
    }
    if (child.stderr) {
      child.stderr.on('data', (data: Buffer) => {
        const handlers = dataListeners.get(id);
        if (handlers) handlers.forEach((h) => h(data.toString()));
      });
    }

    child.on('exit', (code: number | null, signal: string | null) => {
      const handlers = exitListeners.get(id);
      if (handlers) handlers.forEach((h) => h(code ?? -1, signal ? 0 : undefined));
      sessions.delete(id);
    });

    processMap.set(id, child);
  }

  const session: ShellSession = {
    id,
    pid: processMap.get(id)?.pid || 0,
    cols,
    rows,
    createdAt: new Date(),
    env,
  };

  sessions.set(id, session);
  return session;
}

// ── Public API ──

export async function createSession(env: Record<string, string> = {}, profileId?: string): Promise<string> {
  const shell = process.env.SHELL || '/bin/zsh';

  // Merge profile env vars if database is available.
  // The frontend passes the profile ID (numeric), so we resolve by ID — not by
  // name. Querying by name was the previous behaviour and silently failed
  // because the id almost never equals the name. See BUG-1 / US25/US26/US29.
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const { getDefaultProfile, injectEnvById, injectEnv, getEncryptionKey } = require('../lib/env-injector');
    const encryptionKey = getEncryptionKey();
    if (profileId) {
      // Explicit profile selected → inject by id.
      await injectEnvById(profileId, env, encryptionKey);
    } else {
      // No profile specified → fall back to the default profile (by name).
      const profile = getDefaultProfile();
      if (profile) {
        await injectEnv(profile.name, env, encryptionKey);
      }
    }
  } catch {
    // env-injector / database unavailable, continue without injection
  }

  const session = createPTYSession(shell, env);
  return session.id;
}

export function write(sessionId: string, data: string): void {
  const handler = writeHandlers.get(sessionId);
  if (handler) handler(data);
}

export function resize(sessionId: string, cols: number, rows: number): void {
  const session = sessions.get(sessionId);
  if (!session) return;
  session.cols = cols;
  session.rows = rows;

  const proc = processMap.get(sessionId);
  if (proc && typeof proc.resize === 'function') {
    proc.resize(cols, rows);
  }
}

export function killSession(sessionId: string): void {
  const handler = killHandlers.get(sessionId);
  if (handler) handler();
  sessions.delete(sessionId);
  dataListeners.delete(sessionId);
  exitListeners.delete(sessionId);
  writeHandlers.delete(sessionId);
  killHandlers.delete(sessionId);
  processMap.delete(sessionId);
}

export function onData(sessionId: string, handler: DataHandler): () => void {
  if (!dataListeners.has(sessionId)) {
    dataListeners.set(sessionId, new Set());
  }
  dataListeners.get(sessionId)!.add(handler);
  return () => {
    dataListeners.get(sessionId)?.delete(handler);
  };
}

export function onExit(sessionId: string, handler: ExitHandler): () => void {
  if (!exitListeners.has(sessionId)) {
    exitListeners.set(sessionId, new Set());
  }
  exitListeners.get(sessionId)!.add(handler);
  return () => {
    exitListeners.get(sessionId)?.delete(handler);
  };
}

export function getActiveSessions(): ShellSession[] {
  return Array.from(sessions.values());
}

export function getSessionCount(): number {
  return sessions.size;
}
