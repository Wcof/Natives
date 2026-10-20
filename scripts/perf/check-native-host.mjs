#!/usr/bin/env node
/** ADR-0023 P0/P3 resource and lifecycle gates for native-file-host. */
import { existsSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { execFileSync, spawn } from 'node:child_process';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
export const BINARY_BUDGET = 4 * 1024 * 1024;
export const RSS_BUDGET_KB = 12 * 1024;

function binaryPath(root, arg) {
  if (arg) return resolve(root, arg);
  if (process.env.NATIVES_HOST_BIN) return resolve(process.env.NATIVES_HOST_BIN);
  return resolve(root, 'target', 'release', process.platform === 'win32' ? 'native-file-host.exe' : 'native-file-host');
}

export function readRssKb(pid) {
  if (process.platform === 'win32') return { status: 'unsupported', reason: 'Windows RSS sampling is not implemented' };
  try {
    const output = execFileSync('ps', ['-o', 'rss=', '-p', String(pid)], { encoding: 'utf8' }).trim();
    const value = Number.parseInt(output, 10);
    return Number.isFinite(value) ? { status: 'ok', kb: value } : { status: 'unsupported', reason: 'ps returned no RSS' };
  } catch (error) {
    return { status: 'unsupported', reason: String(error) };
  }
}

function frame(value) {
  const body = Buffer.from(JSON.stringify(value));
  const header = Buffer.alloc(4);
  header.writeUInt32LE(body.length);
  return Buffer.concat([header, body]);
}

function waitForResponse(child, timeoutMs = 3500) {
  return new Promise((resolvePromise, reject) => {
    let data = Buffer.alloc(0);
    const timer = setTimeout(() => reject(new Error('host response timeout')), timeoutMs);
    child.stdout.on('data', (chunk) => {
      data = Buffer.concat([data, chunk]);
      // Parse every complete frame and retain leftovers: a coalesced read may
      // contain an unsolicited connected/event frame followed by the response.
      while (data.length >= 4) {
        const size = data.readUInt32LE(0);
        if (data.length < size + 4) break;
        const frame = data.subarray(4, size + 4);
        data = data.subarray(size + 4);
        try {
          const value = JSON.parse(frame.toString());
          // ADR-0042 relays may emit an unsolicited connected/event frame
          // before the response. Only a protocol response carries id + ok.
          if (value && typeof value.id === 'string' && typeof value.ok === 'boolean') {
            clearTimeout(timer);
            resolvePromise(value);
            return;
          }
        } catch (e) { clearTimeout(timer); reject(e); return; }
      }
    });
    child.once('error', (error) => { clearTimeout(timer); reject(error); });
  });
}

async function rootsAndRss(bin, env) {
  const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'pipe'], env });
  try {
    child.stdin.write(frame({ id: 'perf-roots', method: 'roots', params: {} }));
    const response = await waitForResponse(child);
    const rss = readRssKb(child.pid);
    return { responseOk: response?.ok === true, rss };
  } finally {
    // The roots probe intentionally keeps stdin open; always reap it before
    // returning, including response timeout/parse failures.
    child.kill();
    if (child.exitCode === null && child.signalCode === null) {
      await new Promise((resolvePromise) => child.once('exit', resolvePromise));
    }
  }
}

async function request(child, value) {
  child.stdin.write(frame(value));
  return waitForResponse(child);
}

async function directoryEvidence(bin, env) {
  const scratch = mkdtempSync(join(tmpdir(), 'natives-host-list-'));
  try {
    for (let i = 0; i < 10_000; i += 1) writeFileSync(join(scratch, `entry-${i}`), '');
    const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'pipe'], env });
    try {
      await request(child, { id: 'perf-roots', method: 'roots', params: {} }); // warmup
      const samples = [];
      for (let i = 0; i < 5; i += 1) {
        const started = performance.now();
        const response = await request(child, {
          id: `perf-list-${i}`,
          method: 'list_dir',
          params: { path: scratch, offset: 0, limit: 100 },
        });
        samples.push(performance.now() - started);
        if (response?.ok !== true) throw new Error('list_dir request failed');
      }
      const sorted = [...samples].sort((a, b) => a - b);
      const p95 = sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1)] ?? 0;
      return { status: 'ok', entries: 10_000, samples: samples.length, p95Ms: p95, budgetMs: 50, ok: p95 <= 50 };
    } finally {
      child.kill();
      if (child.exitCode === null && child.signalCode === null) await new Promise((done) => child.once('exit', done));
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

export function eofExit(bin, timeoutMs = 2000, env = process.env) {
  return new Promise((resolvePromise, reject) => {
    const started = Date.now();
    let settled = false;
    const child = spawn(bin, [], { stdio: ['pipe', 'ignore', 'ignore'], env });
    const timer = setTimeout(() => { if (!settled) { child.kill(); reject(new Error(`host did not exit within ${timeoutMs}ms`)); } }, timeoutMs + 250);
    child.once('error', (error) => { settled = true; clearTimeout(timer); reject(error); });
    child.once('exit', (code, signal) => { settled = true; clearTimeout(timer); resolvePromise({ ms: Date.now() - started, code, signal, ok: Date.now() - started <= timeoutMs }); });
    child.stdin.end();
  });
}

async function startIsolatedService(bin, root) {
  mkdirSync(join(root, '.tmp'), { recursive: true });
  const dataRoot = mkdtempSync(join(root, '.tmp', 'native-host-perf-'));
  const port = await new Promise((resolvePort, reject) => {
    const probe = net.createServer();
    probe.once('error', reject);
    probe.listen(0, '127.0.0.1', () => {
      const selected = probe.address().port;
      probe.close(() => resolvePort(selected));
    });
  });
  const env = {
    ...process.env,
    NATIVES_WEB_PORT: String(port),
    NATIVES_STATE_DIR: join(dataRoot, 'state'),
    NATIVES_WORKSPACE_DB: join(dataRoot, 'workspace.db'),
    NATIVES_APPS_ROOT: join(dataRoot, 'apps'),
  };
  const child = spawn(bin, ['--service', '--port', String(port)], { env, stdio: 'ignore' });
  try {
    let ready = false;
    for (let attempt = 0; attempt < 60; attempt += 1) {
      ready = await fetch(`http://127.0.0.1:${port}/healthz`)
        .then((response) => response.ok)
        .catch(() => false);
      if (ready) break;
      if (child.exitCode !== null) break;
      await new Promise((done) => setTimeout(done, 50));
    }
    if (!ready) throw new Error('isolated native-file-host service failed to start');
    return { child, dataRoot, env };
  } catch (error) {
    await stopIsolatedService({ child, dataRoot });
    throw error;
  }
}

async function stopIsolatedService({ child, dataRoot }) {
  child.kill('SIGTERM');
  if (child.exitCode === null && child.signalCode === null) {
    await new Promise((done) => {
      const timeout = setTimeout(() => child.kill('SIGKILL'), 2000);
      child.once('exit', () => { clearTimeout(timeout); done(); });
    });
  }
  rmSync(dataRoot, { recursive: true, force: true });
}

export async function runNativeHostCheck(root = ROOT, arg) {
  const bin = binaryPath(root, arg);
  // Cargo checks the whole dependency graph; a single main.rs timestamp misses changes.
  if (!arg && !process.env.NATIVES_HOST_BIN) {
    execFileSync('rtk', ['env', '-u', 'CARGO_TARGET_DIR', '-u', 'CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS', 'cargo', 'build', '-p', 'native-file-host', '--release'], { cwd: root, stdio: 'inherit' });
  }
  if (!existsSync(bin)) return { ok: false, binary: bin, error: 'binary missing' };
  const isolated = !arg && !process.env.NATIVES_HOST_BIN ? await startIsolatedService(bin, root) : null;
  try {
    const env = isolated?.env ?? process.env;
    const bytes = statSync(bin).size;
    const roots = await rootsAndRss(bin, env);
    const eof = await eofExit(bin, 2000, env);
    let directory;
    try { directory = await directoryEvidence(bin, env); } catch (error) {
      directory = { status: 'error', error: String(error), budgetMs: 50, ok: false };
    }
    return {
      ok: bytes <= BINARY_BUDGET && roots.responseOk && roots.rss.status === 'ok' && roots.rss.kb <= RSS_BUDGET_KB && eof.ok && directory.ok,
      binary: bin,
      bytes,
      binaryBudget: BINARY_BUDGET,
      roots,
      eof,
      directory,
      cpu: { status: 'unsupported', reason: 'portable idle CPU averaging is unavailable in this gate' },
      gpu: { status: 'unsupported', reason: 'no portable GPU-context probe' },
    };
  } finally {
    if (isolated) await stopIsolatedService(isolated);
  }
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const summary = await runNativeHostCheck(ROOT, process.argv[2]);
  console.log(JSON.stringify(summary, null, 2));
  process.exitCode = summary.ok ? 0 : 1;
}
