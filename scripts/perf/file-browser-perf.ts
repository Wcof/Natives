/**
 * scripts/perf/file-browser-perf.ts
 *
 * T02 perf baseline — file-browser benchmark harness.
 *
 * Measures, entirely in memory (no real Tauri host), the operations that dominate
 * the file-browser interaction path and emits `scripts/perf/results/baseline.json`.
 *
 *   npx tsx scripts/perf/file-browser-perf.ts
 *
 * Reported ops (every op entry carries samples / p50 / p95 / DOM / RSS):
 *   list_dir_construct_{1k|5k|10k|50k}  simulated IPC serialization + deserialization round-trip
 *   dom_construct_{1k|5k|10k|50k}       simulated DOM node count: all-N rows vs 200-row window (R-P4)
 *   click_select_delay_200ms            click→selection with an artificial 200 ms fixed delay
 *   click_select_no_delay               click→selection after removing the delay
 *   host_io_sync_{n}x1ms                sequential blocking host reads (main-thread stall)
 *   host_io_async_{n}x1ms               concurrent async host reads (no main-thread stall)
 *   watch_modify_apply_{1k|5k}          apply + coalesce a deep watch-modify stream (R-P6)
 *
 * Budgets from docs/standards/technical/04-performance.md:
 *   click / input / selection feedback  p95 ≤ 100 ms
 *   hot IPC                             p95 ≤  50 ms
 *   > 200 UI items must not create full DOM (R-P4)
 */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import {
  DEFAULT_SEED,
  DIR_SIZES,
  IMAGE_COUNT,
  WATCH_SIZES,
  WATCH_WINDOW_MS,
  WATCH_DEPTH,
  buildFixtureSet,
  type DirEntry,
  type WatchEvent,
} from './generate-fixtures';

const THIS_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(THIS_DIR, '..', '..');
const RESULTS_DIR = path.join(THIS_DIR, 'results');
const RESULTS_FILE = path.join(RESULTS_DIR, 'baseline.json');

const CLICK_BUDGET_MS = 100;
const IPC_BUDGET_MS = 50;
const WINDOW_ROWS = 200;
const NODES_PER_ROW = 8;
const CLICK_DELAY_MS = 200;
const CLICK_DELAY_SAMPLES = 12;
const CLICK_DIRECT_SAMPLES = 20;
const IO_BATCH = 20;
const IO_OP_MS = 1;
const IO_SAMPLES = 8;

// ---- statistics ------------------------------------------------------------

interface OpStat {
  samples: number;
  p50: number;
  p95: number;
  min: number;
  max: number;
  mean: number;
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil(p * sorted.length) - 1));
  return sorted[idx] ?? 0;
}

function stats(samples: number[]): OpStat {
  const s = [...samples].sort((a, b) => a - b);
  const sum = samples.reduce((acc, v) => acc + v, 0);
  return {
    samples: s.length,
    p50: percentile(s, 0.5),
    p95: percentile(s, 0.95),
    min: s[0] ?? 0,
    max: s[s.length - 1] ?? 0,
    mean: s.length > 0 ? sum / s.length : 0,
  };
}

// ---- simulated DOM tree (node count, R-P4) ---------------------------------

interface DomNode {
  tag: string;
  text?: string;
  childNodes: DomNode[];
}

function textNode(text: string): DomNode {
  return { tag: '#text', text, childNodes: [] };
}

function rowDom(item: DirEntry): DomNode {
  return {
    tag: 'li',
    childNodes: [
      { tag: 'input', childNodes: [] },
      { tag: 'span', childNodes: [] },
      {
        tag: 'div',
        childNodes: [
          { tag: 'span', childNodes: [textNode(item.name)] },
          { tag: 'span', childNodes: [textNode(`${item.kind} ${item.size}B`)] },
        ],
      },
    ],
  };
}

function countNodes(node: DomNode): number {
  return 1 + node.childNodes.reduce((acc, child) => acc + countNodes(child), 0);
}

function buildNaiveList(entries: DirEntry[]): DomNode {
  return { tag: 'ul', childNodes: entries.map(rowDom) };
}

function buildWindowedList(entries: DirEntry[], windowRows: number): DomNode {
  const visible = entries.slice(0, windowRows).map(rowDom);
  return {
    tag: 'ul',
    childNodes: [
      { tag: '#spacer-top', text: 'start spacer', childNodes: [] },
      ...visible,
      { tag: '#spacer-bottom', text: 'end spacer', childNodes: [] },
    ],
  };
}

interface DomReport {
  naiveNodes: number;
  windowedNodes: number;
  naiveRows: number;
  windowedRows: number;
  nodesPerRow: number;
  windowSize: number;
  r4Windowed: boolean;
}

// ---- individual benchmarks -------------------------------------------------

function benchListDirConstruct(entries: DirEntry[], samples: number): number[] {
  const out: number[] = [];
  for (let s = 0; s < samples; s++) {
    const t0 = performance.now();
    const wire = JSON.stringify(entries); // host → renderer: serialize
    JSON.parse(wire) as DirEntry[]; // renderer: deserialize
    out.push(performance.now() - t0);
  }
  return out;
}

function benchDomConstruct(entries: DirEntry[], samples: number): { times: number[]; report: DomReport } {
  const times: number[] = [];
  for (let s = 0; s < samples; s++) {
    const t0 = performance.now();
    buildNaiveList(entries);
    times.push(performance.now() - t0);
  }
  const naive = buildNaiveList(entries);
  const windowed = buildWindowedList(entries, WINDOW_ROWS);
  return {
    times,
    report: {
      naiveNodes: countNodes(naive),
      windowedNodes: countNodes(windowed),
      naiveRows: entries.length,
      windowedRows: WINDOW_ROWS,
      nodesPerRow: NODES_PER_ROW,
      windowSize: WINDOW_ROWS,
      r4Windowed: entries.length > WINDOW_ROWS,
    },
  };
}

function benchClickSelection(entries: DirEntry[], withDelay: boolean): Promise<number[]> {
  return new Promise((resolve) => {
    const out: number[] = [];
    const samples = withDelay ? CLICK_DELAY_SAMPLES : CLICK_DIRECT_SAMPLES;
    let done = 0;
    for (let s = 0; s < samples; s++) {
      const t0 = performance.now();
      const finish = () => {
        out.push(performance.now() - t0);
        done += 1;
        if (done === samples) resolve(out);
      };
      if (withDelay) {
        setTimeout(finish, CLICK_DELAY_MS);
      } else {
        // direct path: synchronous selection update, then report
        const item = entries[s % entries.length] ?? entries[0];
        if (item) item.name;
        finish();
      }
    }
  });
}

const WAIT_SAB = new SharedArrayBuffer(4);
const WAIT_VIEW = new Int32Array(WAIT_SAB);

function blockingRead(ms: number): void {
  Atomics.wait(WAIT_VIEW, 0, 0, ms);
}

function asyncRead(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function benchHostIoSync(samples: number): number[] {
  const out: number[] = [];
  for (let s = 0; s < samples; s++) {
    const t0 = performance.now();
    for (let i = 0; i < IO_BATCH; i++) blockingRead(IO_OP_MS);
    out.push(performance.now() - t0);
  }
  return out;
}

async function benchHostIoAsync(samples: number): Promise<number[]> {
  const out: number[] = [];
  for (let s = 0; s < samples; s++) {
    const t0 = performance.now();
    await Promise.all(Array.from({ length: IO_BATCH }, () => asyncRead(IO_OP_MS)));
    out.push(performance.now() - t0);
  }
  return out;
}

function applyWatchModify(events: WatchEvent[]): number {
  const applied = new Map<string, number>();
  for (const e of events) applied.set(e.path, e.ts);
  return applied.size;
}

function benchWatchApply(events: WatchEvent[], samples: number): number[] {
  const out: number[] = [];
  for (let s = 0; s < samples; s++) {
    const t0 = performance.now();
    applyWatchModify(events);
    out.push(performance.now() - t0);
  }
  return out;
}

// ---- report assembly -------------------------------------------------------

interface OpResult extends OpStat {
  op: string;
  DOM: number | DomReport;
  RSS: number;
  verdict: string;
}

function mkOp(op: string, times: number[], dom: number | DomReport, verdict: string): OpResult {
  const st = stats(times);
  return { op, ...st, DOM: dom, RSS: process.memoryUsage().rss, verdict };
}

function getGitSha(): string {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: REPO_ROOT, encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

async function main(): Promise<void> {
  const fixtures = buildFixtureSet(DEFAULT_SEED);
  const ops: Record<string, OpResult> = {};

  // a) list_dir data construction (simulated IPC serialize + deserialize)
  for (const size of DIR_SIZES) {
    const entries = fixtures.dirEntries[size] ?? [];
    const samples = size >= 50000 ? 3 : size >= 10000 ? 5 : 8;
    const times = benchListDirConstruct(entries, samples);
    const p95 = stats(times).p95;
    const verdict = p95 <= IPC_BUDGET_MS ? `pass (≤ ${IPC_BUDGET_MS}ms hot IPC)` : `over hot-IPC budget (${IPC_BUDGET_MS}ms)`;
    ops[`list_dir_construct_${size}`] = mkOp(`list_dir_construct_${size}`, times, 0, verdict);
  }

  // b) DOM construction count (R-P4: >200 items must be windowed)
  for (const size of DIR_SIZES) {
    const entries = fixtures.dirEntries[size] ?? [];
    const samples = size >= 50000 ? 3 : size >= 10000 ? 3 : 5;
    const { times, report } = benchDomConstruct(entries, samples);
    const verdict = report.naiveRows > WINDOW_ROWS
      ? `naive DOM (${report.naiveNodes} nodes) > 200-row window (${report.windowedNodes} nodes) → must window`
      : 'ok';
    ops[`dom_construct_${size}`] = mkOp(`dom_construct_${size}`, times, report, verdict);
  }

  // c) click→selection latency, before/after removing a fixed 200ms delay
  const clickEntries = fixtures.dirEntries[1000] ?? [];
  ops['click_select_delay_200ms'] = mkOp(
    'click_select_delay_200ms',
    await benchClickSelection(clickEntries, true),
    0,
    `fixed ${CLICK_DELAY_MS}ms delay present — expected p95 > ${CLICK_BUDGET_MS}ms budget; remove the delay`,
  );
  ops['click_select_no_delay'] = mkOp(
    'click_select_no_delay',
    await benchClickSelection(clickEntries, false),
    0,
    `delay removed — p95 should be ≤ ${CLICK_BUDGET_MS}ms`,
  );

  // d) Host IO sync vs async wall-clock (simulated, no real Tauri)
  ops['host_io_sync_20x1ms'] = mkOp(
    'host_io_sync_20x1ms',
    benchHostIoSync(IO_SAMPLES),
    0,
    `${IO_BATCH} sequential blocking reads — main thread stalls (R-P2)`,
  );
  ops['host_io_async_20x1ms'] = mkOp(
    'host_io_async_20x1ms',
    await benchHostIoAsync(IO_SAMPLES),
    0,
    `${IO_BATCH} concurrent async reads — wall-clock ≪ sync, no main-thread stall`,
  );

  // watch modify stream apply (R-P6 batching/coalescing)
  for (const size of WATCH_SIZES) {
    const events = fixtures.watchModify[size] ?? [];
    const times = benchWatchApply(events, 8);
    ops[`watch_modify_apply_${size}`] = mkOp(
      `watch_modify_apply_${size}`,
      times,
      0,
      `${events.length} deep modify events in ${WATCH_WINDOW_MS}ms window coalesced into state map`,
    );
  }

  // ---- machine / build / dataset -------------------------------------------
  const cpu = os.cpus()[0];
  const machine = {
    hostname: os.hostname(),
    platform: os.platform(),
    arch: os.arch(),
    release: os.release(),
    cpu: cpu ? `${cpu.model} (${os.cpus().length} cores)` : 'unknown',
    totalmemGB: Number((os.totalmem() / 1024 ** 3).toFixed(1)),
  };
  const build = `node ${process.version} / tsx (in-memory simulation, no real Tauri host)`;
  const dataset = `deterministic seeded fixtures (seed=${DEFAULT_SEED}): dir entries ${DIR_SIZES.join('/')}, images ${IMAGE_COUNT}, watch modify ${WATCH_SIZES.join('/')} in ${WATCH_WINDOW_MS}ms window, depth ${WATCH_DEPTH}`;

  const report = {
    gitSha: getGitSha(),
    machine,
    build,
    dataset,
    generatedAt: new Date().toISOString(),
    budgetMs: {
      clickSelect: CLICK_BUDGET_MS,
      hotIpc: IPC_BUDGET_MS,
      windowRows: WINDOW_ROWS,
      clickDelayToRemove: CLICK_DELAY_MS,
    },
    ops,
  };

  fs.mkdirSync(RESULTS_DIR, { recursive: true });
  fs.writeFileSync(RESULTS_FILE, `${JSON.stringify(report, null, 2)}\n`);

  // ---- human-readable summary ---------------------------------------------
  const line = (row: string[]) => row.map((c, i) => c.padEnd(i === 0 ? 30 : 14)).join('');
  console.log('=== T02 file-browser perf baseline ===');
  console.log(`gitSha   ${report.gitSha}`);
  console.log(`machine  ${machine.hostname} ${machine.platform} ${machine.arch} ${machine.release}`);
  console.log(`dataset  ${report.dataset}`);
  console.log('');
  console.log(line(['op', 'samples', 'p50(ms)', 'p95(ms)', 'RSS(MB)']));
  for (const op of Object.keys(ops)) {
    const r = ops[op] as OpResult;
    const dom =
      typeof r.DOM === 'object'
        ? `naive=${r.DOM.naiveNodes} win=${r.DOM.windowedNodes}`
        : '-';
    console.log(line([op, String(r.samples), r.p50.toFixed(3), r.p95.toFixed(3), (r.RSS / 1024 / 1024).toFixed(1)]) + dom);
  }
  console.log('');
  for (const op of Object.keys(ops)) {
    const r = ops[op] as OpResult;
    console.log(`- ${op}: ${r.verdict}`);
  }
  console.log('');
  console.log(`JSON written to ${RESULTS_FILE}`);
}

main()
  .then(() => {
    process.exitCode = 0;
  })
  .catch((err) => {
    console.error('[file-browser-perf] benchmark failed:', err);
    process.exitCode = 1;
  });
