#!/usr/bin/env node
// PERF-04 — Resource lifecycle soak harness.
//
// Samples the Natives main process, WebView children, and the Agent Daemon
// sidecar on a fixed cadence for a configurable duration, then reports whether
// RSS grew monotonically in the final window and whether any child processes
// were orphaned on exit.
//
// Acceptance (plan PERF-04):
//   - 后 20 分钟 RSS 无持续单调增长
//   - 空闲 CPU p75 ≤ 2%
//   - 关闭窗口后无孤儿子进程
//
// Usage:
//   node scripts/perf/resource-soak.mjs --duration 1800          # 30 min soak
//   node scripts/perf/resource-soak.mjs --interval 30            # sample every 30 s
//   node scripts/perf/resource-soak.mjs --bin ./path/to/natives  # launch the app
//
// The harness launches the binary itself, idles for the duration, sends a quit
// signal, and verifies child reaping. It does NOT drive UI interactions (those
// belong to an end-to-end runner); this is an idle + lifecycle soak.
//
// Exit 0 if RSS is stable in the final window, idle CPU p75 ≤ budget, and no
// orphaned children remain; 1 otherwise.

import { spawn, spawnSync, execSync } from 'node:child_process';
import { existsSync, appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { hostname, platform, freemem, totalmem } from 'node:os';
import path from 'node:path';

const argv = process.argv.slice(2);
function arg(name, fallback) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : fallback;
}
const DURATION_S = Number(arg('duration', '1800'));
const INTERVAL_S = Number(arg('interval', '30'));
const CPU_BUDGET = Number(arg('cpuBudget', '2'));
const EXPLICIT_BIN = arg('bin', null);

const repoRoot = path.resolve(new URL('.', import.meta.url).pathname, '..', '..');
function resolveBinary() {
  if (EXPLICIT_BIN) return EXPLICIT_BIN;
  const candidates = [
    path.join(repoRoot, 'src-tauri/target/release/natives'),
    path.join(repoRoot, 'src-tauri/target/release/Natives'),
    '/Applications/Natives.app/Contents/MacOS/natives',
  ];
  for (const c of candidates) if (existsSync(c)) return c;
  return null;
}

const BIN = resolveBinary();
if (!BIN || !existsSync(BIN)) {
  console.error('resource-soak: release binary not found (build via `npm run tauri:build` or pass --bin).');
  process.exit(2);
}

function pidAlive(pid) {
  try {
    process.kill(Number(pid), 0);
    return true;
  } catch {
    return false;
  }
}

// Collect the main PID plus descendant processes (WebView renderers, Helper,
// Agent Daemon sidecar). Uses `pgrep -P` walking on macOS/Linux.
function collectDescendants(rootPid) {
  const all = [rootPid];
  let frontier = [rootPid];
  const seen = new Set([rootPid]);
  for (let depth = 0; depth < 6 && frontier.length; depth += 1) {
    const next = [];
    for (const pid of frontier) {
      try {
        const out = execSync(`pgrep -P ${pid}`, { stdio: ['ignore', 'pipe', 'ignore'] }).toString();
        for (const line of out.split('\n')) {
          const c = line.trim();
          if (!c) continue;
          const child = Number(c);
          if (child && !seen.has(child)) {
            seen.add(child);
            all.push(child);
            next.push(child);
          }
        }
      } catch {
        /* no children or pgrep unavailable */
      }
    }
    frontier = next;
  }
  return all;
}

// ps columns: pid, rss (KB), %cpu, threads (STATE/...) — portable-ish.
function samplePs(pids) {
  const procs = [];
  for (const pid of pids) {
    if (!pidAlive(pid)) continue;
    try {
      const cols = platform() === 'darwin' ? 'pid,rss,%cpu' : 'pid,rss,%cpu';
      const out = execSync(`ps -o ${cols} -p ${pid} -h`, { stdio: ['ignore', 'pipe', 'ignore'] })
        .toString()
        .trim();
      if (!out) continue;
      const parts = out.split(/\s+/);
      if (parts.length >= 3) {
        procs.push({ pid: Number(parts[0]), rssKb: Number(parts[1]), cpuPct: Number(parts[2]) });
      }
    } catch {
      /* process gone */
    }
  }
  return procs;
}

function machineFingerprint() {
  let gitSha = 'unknown';
  try { gitSha = execSync('git rev-parse --short HEAD', { encoding: 'utf8' }).trim(); } catch {}
  return { hostname: hostname(), platform: platform(), totalmem: totalmem(), gitSha, binary: BIN };
}

const samples = [];
const dir = path.resolve(new URL('.', import.meta.url).pathname);
const outFile = path.join(dir, 'results', 'resource-soak.json');
const csvFile = path.join(dir, 'results', 'resource-soak.csv');
mkdirSync(path.join(dir, 'results'), { recursive: true });

console.log(`resource-soak: ${DURATION_S}s @ ${INTERVAL_S}s interval, cpu p75 budget ≤ ${CPU_BUDGET}%`);
console.log(`binary: ${BIN}`);

const child = spawn(BIN, [], {
  env: process.env,
  stdio: ['ignore', 'ignore', 'ignore'],
  detached: false,
});

// Give it a moment to fork children before the first sample.
await new Promise((r) => setTimeout(r, 3000));
if (!pidAlive(child.pid)) {
  console.error('resource-soak: process exited immediately; aborting.');
  process.exit(1);
}
const rootPid = child.pid;
console.log(`launched main pid=${rootPid}`);

appendFileSync(csvFile, 't_s,total_rss_mb,proc_count,max_cpu_pct,sum_cpu_pct\n');

const startMs = Date.now();
let elapsed = 0;
while (elapsed < DURATION_S) {
  const t = Math.round((Date.now() - startMs) / 1000);
  const pids = collectDescendants(rootPid);
  const procs = samplePs(pids);
  const totalRssMb = procs.reduce((a, p) => a + p.rssKb, 0) / 1024;
  const maxCpu = procs.reduce((a, p) => Math.max(a, p.cpuPct), 0);
  const sumCpu = procs.reduce((a, p) => a + p.cpuPct, 0);
  samples.push({ t, totalRssMb, procCount: pids.length, maxCpu, sumCpu });
  appendFileSync(csvFile, `${t},${totalRssMb.toFixed(1)},${pids.length},${maxCpu.toFixed(2)},${sumCpu.toFixed(2)}\n`);
  process.stdout.write(`t=${t}s rss=${totalRssMb.toFixed(0)}MB procs=${pids.length} cpuΣ=${sumCpu.toFixed(1)}%\r`);
  await new Promise((r) => setTimeout(r, INTERVAL_S * 1000));
  elapsed = Math.round((Date.now() - startMs) / 1000);
}

// Tear down and check for orphans.
const descendantsAtQuit = collectDescendants(rootPid);
try {
  if (platform() === 'darwin') {
    // Prefer an app-level quit signal so the Host runs its shutdown_all_processes.
    execSync(`osascript -e 'tell application "Natives" to quit'`, { stdio: 'ignore' });
  }
} catch { /* fall through to SIGTERM */ }

await new Promise((r) => setTimeout(r, 3000));
if (pidAlive(rootPid)) {
  try { process.kill(rootPid, 'SIGTERM'); } catch {}
  await new Promise((r) => setTimeout(r, 2000));
}
if (pidAlive(rootPid)) {
  try { process.kill(rootPid, 'SIGKILL'); } catch {}
}

// Orphan check: any descendant that outlived the root.
await new Promise((r) => setTimeout(r, 2000));
const orphans = descendantsAtQuit.filter((pid) => pid !== rootPid && pidAlive(pid));

// ── Analysis ──
function percentile(sorted, p) {
  if (!sorted.length) return 0;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[idx];
}

// Idle CPU: average of sumCpu across all samples (the process tree should be idle).
const cpuSamples = samples.map((s) => s.sumCpu).sort((a, b) => a - b);
const cpuP75 = percentile(cpuSamples, 75);
const cpuAvg = cpuSamples.reduce((a, b) => a + b, 0) / (cpuSamples.length || 1);

// RSS stability in the final 20 minutes (or final third if shorter).
const tailLen = Math.max(2, Math.min(samples.length, Math.round(DURATION_S * 20 / 60 / INTERVAL_S) || Math.ceil(samples.length / 3)));
const tail = samples.slice(-tailLen);
const rssTail = tail.map((s) => s.totalRssMb);
const rssMonotonicUp = rssTail.every((v, i) => i === 0 || v >= rssTail[i - 1] - 0.5);
const rssDelta = rssTail.length ? rssTail[rssTail.length - 1] - rssTail[0] : 0;
// "No sustained monotonic growth": allow small noise but reject a steady climb.
const rssGrowing = rssMonotonicUp && rssDelta > 5;

const result = {
  machine: machineFingerprint(),
  durationS: DURATION_S,
  intervalS: INTERVAL_S,
  cpuBudget: CPU_BUDGET,
  cpuP75,
  cpuAvg,
  rssTailMb: rssTail,
  rssDeltaMb: rssDelta,
  rssMonotonicUp,
  rssGrowing,
  orphans,
  samples,
  generatedAt: new Date().toISOString(),
};

console.log('\n' + '─'.repeat(60));
console.log(`idle cpu: avg=${cpuAvg.toFixed(2)}%  p75=${cpuP75.toFixed(2)}%  (budget ${CPU_BUDGET}%)`);
console.log(`rss tail (${tailLen} samples): start=${rssTail[0]?.toFixed(0)}MB end=${rssTail[rssTail.length - 1]?.toFixed(0)}MB  delta=${rssDelta.toFixed(1)}MB`);
console.log(`rss sustained monotonic growth: ${rssGrowing ? 'YES (FAIL)' : 'no'}`);
console.log(`orphaned children after quit: ${orphans.length} ${orphans.length ? orphans.join(',') : ''}`);

const cpuOk = cpuP75 <= CPU_BUDGET;
const rssOk = !rssGrowing;
const orphanOk = orphans.length === 0;
const verdict = cpuOk && rssOk && orphanOk ? 'PASS' : 'FAIL';
console.log('─'.repeat(60));
console.log(`verdict: ${verdict}  (cpu ${cpuOk ? 'ok' : 'FAIL'}, rss ${rssOk ? 'ok' : 'FAIL'}, orphans ${orphanOk ? 'ok' : 'FAIL'})`);

writeFileSync(outFile, JSON.stringify(result, null, 2));
console.log(`written: ${outFile}`);
console.log(`csv: ${csvFile}`);

process.exit(verdict === 'PASS' ? 0 : 1);
