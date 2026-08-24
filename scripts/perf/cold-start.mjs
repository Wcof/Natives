#!/usr/bin/env node
// PERF-01 — Release cold-start phase timing harness.
//
// Runs the published Natives binary N times with NATIVES_STARTUP_TIMING=1,
// captures the [natives.startup] phase table each run, and reports p75 of the
// total "window_ready" time vs the project budget (p75 ≤ 2500 ms).
//
// Reproducibility (R-P1): same machine, same build, same dataset (the user's
// ~/.natives store). Machine fingerprint is recorded in the output. Only compare
// runs on the same machine + build.
//
// Usage:
//   node scripts/perf/cold-start.mjs                       # 5 runs, auto-detect binary
//   node scripts/perf/cold-start.mjs --runs 8              # custom run count
//   node scripts/perf/cold-start.mjs --bin ./target/...    # explicit binary path
//   node scripts/perf/cold-start.mjs --budget 2500         # override budget ms
//
// Exit code 0 if observed p75 ≤ budget, 1 otherwise (CI gate).

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { hostname, platform, arch, cpus, totalmem } from 'node:os';
import { execSync } from 'node:child_process';
import path from 'node:path';

const argv = process.argv.slice(2);
function arg(name, fallback) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : fallback;
}
const RUNS = Number(arg('runs', '5'));
const BUDGET_MS = Number(arg('budget', '2500'));
const EXPLICIT_BIN = arg('bin', null);

if (!Number.isInteger(RUNS) || RUNS < 1) {
  console.error(`invalid --runs value: ${RUNS}`);
  process.exit(2);
}

// ── Locate the release binary ──
function resolveBinary() {
  if (EXPLICIT_BIN) return EXPLICIT_BIN;
  const repoRoot = path.resolve(new URL('.', import.meta.url).pathname, '..', '..');
  // 1. tauri build output (release)
  const candidates = [
    path.join(repoRoot, 'src-tauri/target/release/natives'),
    path.join(repoRoot, 'src-tauri/target/release/Natives'),
    '/Applications/Natives.app/Contents/MacOS/natives',
    '/Applications/Natives.app/Contents/MacOS/Natives',
  ];
  for (const c of candidates) {
    if (existsSync(c)) return c;
  }
  return null;
}

const BIN = resolveBinary();
if (!BIN || !existsSync(BIN)) {
  console.error(
    'cold-start: release binary not found.\n' +
      'Build it first:  rtk npm run tauri:build\n' +
      'or pass --bin <path>',
  );
  process.exit(2);
}

function machineFingerprint() {
  let gitSha = 'unknown';
  try {
    gitSha = execSync('git rev-parse --short HEAD', { encoding: 'utf8' }).trim();
  } catch {
    /* detached / not a repo */
  }
  return {
    hostname: hostname(),
    platform: platform(),
    arch: arch(),
    cpu: cpus()[0]?.model ?? 'unknown',
    cpuCount: cpus().length,
    totalmem: totalmem(),
    gitSha,
    binary: BIN,
  };
}

// Parse one run's stderr for the [natives.startup] phase table.
function parseStartupTimings(stderr) {
  const lines = stderr.split('\n');
  const phases = [];
  let totalUntilReport = null;
  for (const line of lines) {
    const phaseMatch = line.match(/\[natives\.startup\]\s+([^\s].*?)\s+(\d+)\s*ms\s*$/);
    if (phaseMatch) {
      const name = phaseMatch[1].trim();
      const millis = Number(phaseMatch[2]);
      phases.push({ name, millis });
      if (name === 'window_ready') {
        // window_ready carries span 0; the real cold-start number is totalUntilReport.
      }
      continue;
    }
    const totalMatch = line.match(/total until report:\s*(\d+)\s*ms/);
    if (totalMatch) {
      totalUntilReport = Number(totalMatch[1]);
    }
  }
  return { phases, totalUntilReport };
}

function percentile(sorted, p) {
  if (sorted.length === 0) return null;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[idx];
}

function runOnce() {
  // Cold start: process must not be running before we launch it. The harness
  // does NOT kill a running instance — caller is responsible for a clean slate.
  const start = Date.now();
  const res = spawnSync(BIN, [], {
    env: { ...process.env, NATIVES_STARTUP_TIMING: '1' },
    encoding: 'utf8',
    timeout: 60_000,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const wallMs = Date.now() - start;
  const parsed = parseStartupTimings(res.stderr || '');
  return { wallMs, parsed, stderr: res.stderr || '', exitCode: res.status };
}

const runs = [];
console.log(`cold-start: ${RUNS} runs, budget p75 ≤ ${BUDGET_MS} ms`);
console.log(`binary: ${BIN}`);
console.log('─'.repeat(60));

for (let i = 0; i < RUNS; i += 1) {
  // Best-effort: ensure no lingering instance between runs.
  try {
    if (platform() === 'darwin') {
      execSync('pkill -f "/Natives.app/|/natives" || true', { stdio: 'ignore' });
    } else if (platform() === 'linux') {
      execSync('pkill -f "natives" || true', { stdio: 'ignore' });
    }
  } catch {
    /* ignore */
  }
  process.stdout.write(`run ${i + 1}/${RUNS} ... `);
  const r = runOnce();
  const total = r.parsed.totalUntilReport;
  runs.push(r);
  if (total == null) {
    console.log('NO TIMING OUTPUT (is NATIVES_STARTUP_TIMING honored by this build?)');
  } else {
    console.log(`window_ready=${total} ms (wall=${r.wallMs} ms, exit=${r.exitCode})`);
  }
}

const valid = runs.filter((r) => r.parsed.totalUntilReport != null);
const totals = valid.map((r) => r.parsed.totalUntilReport).sort((a, b) => a - b);

// Aggregate per-phase medians across runs.
const phaseAgg = {};
for (const r of valid) {
  for (const ph of r.parsed.phases) {
    if (ph.name === 'window_ready') continue;
    (phaseAgg[ph.name] ??= []).push(ph.millis);
  }
}
const phaseSummary = Object.fromEntries(
  Object.entries(phaseAgg).map(([name, arr]) => {
    const s = [...arr].sort((a, b) => a - b);
    return [
      name,
      { samples: arr.length, p50: percentile(s, 50), p75: percentile(s, 75), max: s[s.length - 1] },
    ];
  }),
);

const result = {
  machine: machineFingerprint(),
  budgetMs: BUDGET_MS,
  runs: RUNS,
  collectedRuns: totals.length,
  p50: percentile(totals, 50),
  p75: percentile(totals, 75),
  max: totals.length ? totals[totals.length - 1] : null,
  phaseSummary,
  generatedAt: new Date().toISOString(),
};

console.log('─'.repeat(60));
if (totals.length === 0) {
  console.error('cold-start: no valid timing output collected across all runs.');
  console.error('Ensure the binary was built from a tree containing startup_timing instrumentation.');
  process.exit(1);
}

console.log(`window_ready p50 = ${result.p50} ms`);
console.log(`window_ready p75 = ${result.p75} ms  (budget ${BUDGET_MS} ms)`);
console.log(`window_ready max = ${result.max} ms`);
console.log('─'.repeat(60));
console.log('phase ranking by p75 (ms):');
const ranked = Object.entries(phaseSummary).sort((a, b) => (b[1].p75 ?? 0) - (a[1].p75 ?? 0));
for (const [name, s] of ranked) {
  console.log(`  ${name.padEnd(22)} p50=${String(s.p50).padStart(5)}  p75=${String(s.p75).padStart(5)}  max=${String(s.max).padStart(5)}`);
}

const verdict = result.p75 != null && result.p75 <= BUDGET_MS ? 'PASS' : 'FAIL';
console.log('─'.repeat(60));
console.log(`verdict: ${verdict}  (p75 ${result.p75} ms vs budget ${BUDGET_MS} ms)`);

const outDir = path.resolve(new URL('.', import.meta.url).pathname);
const outFile = path.join(outDir, 'results', 'cold-start.json');
try {
  const { mkdirSync, writeFileSync } = await import('node:fs');
  mkdirSync(path.join(outDir, 'results'), { recursive: true });
  writeFileSync(outFile, JSON.stringify(result, null, 2));
  console.log(`written: ${outFile}`);
} catch (e) {
  console.error(`could not write ${outFile}: ${e.message}`);
}

process.exit(verdict === 'PASS' ? 0 : 1);
