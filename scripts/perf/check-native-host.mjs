#!/usr/bin/env node
/** ADR-0023 P0/P3 resource and lifecycle gates for native-file-host. */
import { existsSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { execFileSync, spawn } from 'node:child_process';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
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

function waitForResponse(child, timeoutMs = 2000) {
  return new Promise((resolvePromise, reject) => {
    let data = Buffer.alloc(0);
    const timer = setTimeout(() => reject(new Error('host response timeout')), timeoutMs);
    child.stdout.on('data', (chunk) => {
      data = Buffer.concat([data, chunk]);
      if (data.length < 4) return;
      const size = data.readUInt32LE(0);
      if (data.length >= size + 4) {
        clearTimeout(timer);
        try { resolvePromise(JSON.parse(data.subarray(4, size + 4).toString())); } catch (e) { reject(e); }
      }
    });
    child.once('error', (error) => { clearTimeout(timer); reject(error); });
  });
}

async function rootsAndRss(bin) {
  const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'pipe'] });
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

async function directoryEvidence(bin) {
  const scratch = mkdtempSync(join(tmpdir(), 'natives-host-list-'));
  try {
    for (let i = 0; i < 10_000; i += 1) writeFileSync(join(scratch, `entry-${i}`), '');
    const child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'pipe'] });
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

export function eofExit(bin, timeoutMs = 2000) {
  return new Promise((resolvePromise, reject) => {
    const started = Date.now();
    let settled = false;
    const child = spawn(bin, [], { stdio: ['pipe', 'ignore', 'ignore'] });
    const timer = setTimeout(() => { if (!settled) { child.kill(); reject(new Error(`host did not exit within ${timeoutMs}ms`)); } }, timeoutMs + 250);
    child.once('error', (error) => { settled = true; clearTimeout(timer); reject(error); });
    child.once('exit', (code, signal) => { settled = true; clearTimeout(timer); resolvePromise({ ms: Date.now() - started, code, signal, ok: Date.now() - started <= timeoutMs }); });
    child.stdin.end();
  });
}

export async function runNativeHostCheck(root = ROOT, arg) {
  const bin = binaryPath(root, arg);
  // Auto-build release binary if missing or stale compared to crates sources
  try {
    const isStale = !existsSync(bin) || statSync(bin).mtimeMs < statSync(join(root, 'crates/native-file-host/src/main.rs')).mtimeMs;
    if (isStale) {
      execFileSync('cargo', ['build', '-p', 'native-file-host', '--release'], { cwd: root, stdio: 'inherit' });
    }
  } catch {}
  if (!existsSync(bin)) return { ok: false, binary: bin, error: 'binary missing' };
  const bytes = statSync(bin).size;
  const roots = await rootsAndRss(bin);
  const eof = await eofExit(bin);
  let directory;
  try { directory = await directoryEvidence(bin); } catch (error) {
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
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const summary = await runNativeHostCheck(ROOT, process.argv[2]);
  console.log(JSON.stringify(summary, null, 2));
  process.exitCode = summary.ok ? 0 : 1;
}
