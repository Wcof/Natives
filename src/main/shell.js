"use strict";
// ── Shell Manager ──
// PTY terminal via node-pty with graceful degradation to child_process.spawn
Object.defineProperty(exports, "__esModule", { value: true });
exports.createSession = createSession;
exports.write = write;
exports.resize = resize;
exports.killSession = killSession;
exports.onData = onData;
exports.onExit = onExit;
exports.getActiveSessions = getActiveSessions;
exports.getSessionCount = getSessionCount;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let ptySpawn = null;
let usePty = true;
try {
    // Dynamic require for optional native module
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const pty = require('node-pty');
    ptySpawn = pty.spawn;
}
catch {
    usePty = false;
}
const sessions = new Map();
const dataListeners = new Map();
const exitListeners = new Map();
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const processMap = new Map(); // raw process reference (IPty or ChildProcess)
const writeHandlers = new Map();
const killHandlers = new Map();
let sessionCounter = 0;
// ── PTY Session ──
function createPTYSession(shell, env) {
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
            writeHandlers.set(id, (data) => ptyProcess.write(data));
            killHandlers.set(id, () => ptyProcess.kill());
            ptyProcess.onData((data) => {
                const handlers = dataListeners.get(id);
                if (handlers)
                    handlers.forEach((h) => h(data));
            });
            ptyProcess.onExit(({ exitCode, signal }) => {
                const handlers = exitListeners.get(id);
                if (handlers)
                    handlers.forEach((h) => h(exitCode, signal));
                sessions.delete(id);
            });
            processMap.set(id, ptyProcess);
        }
        catch {
            // PTY spawn failed, fall through to child_process fallback
            usePty = false;
            return createPTYSession(shell, env);
        }
    }
    else {
        // Fallback to child_process.spawn
        // eslint-disable-next-line @typescript-eslint/no-require-imports
        const { spawn } = require('child_process');
        const child = spawn(shell, [], {
            env: { ...process.env, ...env },
            cwd: process.env.HOME,
        });
        writeHandlers.set(id, (data) => {
            if (child.stdin?.writable)
                child.stdin.write(data);
        });
        killHandlers.set(id, () => child.kill());
        if (child.stdout) {
            child.stdout.on('data', (data) => {
                const handlers = dataListeners.get(id);
                if (handlers)
                    handlers.forEach((h) => h(data.toString()));
            });
        }
        if (child.stderr) {
            child.stderr.on('data', (data) => {
                const handlers = dataListeners.get(id);
                if (handlers)
                    handlers.forEach((h) => h(data.toString()));
            });
        }
        child.on('exit', (code, signal) => {
            const handlers = exitListeners.get(id);
            if (handlers)
                handlers.forEach((h) => h(code ?? -1, signal ? 0 : undefined));
            sessions.delete(id);
        });
        processMap.set(id, child);
    }
    const session = {
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
async function createSession(env = {}, profileId) {
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
        }
        else {
            // No profile specified → fall back to the default profile (by name).
            const profile = getDefaultProfile();
            if (profile) {
                await injectEnv(profile.name, env, encryptionKey);
            }
        }
    }
    catch {
        // env-injector / database unavailable, continue without injection
    }
    const session = createPTYSession(shell, env);
    return session.id;
}
function write(sessionId, data) {
    const handler = writeHandlers.get(sessionId);
    if (handler)
        handler(data);
}
function resize(sessionId, cols, rows) {
    const session = sessions.get(sessionId);
    if (!session)
        return;
    session.cols = cols;
    session.rows = rows;
    const proc = processMap.get(sessionId);
    if (proc && typeof proc.resize === 'function') {
        proc.resize(cols, rows);
    }
}
function killSession(sessionId) {
    const handler = killHandlers.get(sessionId);
    if (handler)
        handler();
    sessions.delete(sessionId);
    dataListeners.delete(sessionId);
    exitListeners.delete(sessionId);
    writeHandlers.delete(sessionId);
    killHandlers.delete(sessionId);
    processMap.delete(sessionId);
}
function onData(sessionId, handler) {
    if (!dataListeners.has(sessionId)) {
        dataListeners.set(sessionId, new Set());
    }
    dataListeners.get(sessionId).add(handler);
    return () => {
        dataListeners.get(sessionId)?.delete(handler);
    };
}
function onExit(sessionId, handler) {
    if (!exitListeners.has(sessionId)) {
        exitListeners.set(sessionId, new Set());
    }
    exitListeners.get(sessionId).add(handler);
    return () => {
        exitListeners.get(sessionId)?.delete(handler);
    };
}
function getActiveSessions() {
    return Array.from(sessions.values());
}
function getSessionCount() {
    return sessions.size;
}
//# sourceMappingURL=shell.js.map