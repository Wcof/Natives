# scripts/perf — T02 file-browser perf baseline

Repeatable, in-memory benchmark harness for the file-browser interaction path.
No real Tauri host is contacted; all IO/IPC is simulated so the harness runs
anywhere with `tsx` (already a devDependency) — **no `npm install` required and
none may be run here** (Coordinator owns `package.json`).

## Files

| path | purpose |
| --- | --- |
| `generate-fixtures.ts` | deterministic seeded fixtures: 1k/5k/10k/50k dir entries, 500 images, 1k/5k deep watch-`modify` streams in a 1 s window |
| `file-browser-perf.ts` | benchmark harness; emits `results/baseline.json` |
| `virtualizer-spike.md` | internal row virtualizer vs @tanstack/react-virtual — recommendation for T01 |
| `results/baseline.json` | latest committed baseline run (evidence, R-P1) |
| `fixtures/` | optional on-disk JSON dumps (gitignored; harness never depends on them) |

## Run

```sh
# from the repo root
npx tsx scripts/perf/file-browser-perf.ts

# or through the rtk shell wrapper used by the agent runtime
rtk npx tsx scripts/perf/file-browser-perf.ts
```

Optional — materialise fixtures as JSON for offline inspection:

```sh
npx tsx scripts/perf/generate-fixtures.ts          # writes scripts/perf/fixtures/*.json
npx tsx scripts/perf/generate-fixtures.ts /tmp/fx  # custom outDir
```

## What is measured

| op | what | budget (04-performance.md) |
| --- | --- | --- |
| `list_dir_construct_{n}` | simulated IPC serialize + deserialize round-trip for n dir entries | hot IPC p95 ≤ 50 ms |
| `dom_construct_{n}` | simulated DOM node count: all-N rows vs 200-row window | >200 items must window (R-P4) |
| `click_select_delay_200ms` | click→selection with an artificial 200 ms fixed delay | p95 ≤ 100 ms (expected to fail → remove delay) |
| `click_select_no_delay` | click→selection after the delay is removed | p95 ≤ 100 ms |
| `host_io_sync_20x1ms` | 20 sequential blocking host reads (main-thread stall) | non-blocking IO (R-P2) |
| `host_io_async_20x1ms` | 20 concurrent async host reads | wall-clock ≪ sync, no stall |
| `watch_modify_apply_{n}` | coalesce a deep watch-`modify` stream into state | bounded apply (R-P6) |

## Output schema

`results/baseline.json` top level: `gitSha / machine / build / dataset /
generatedAt / budgetMs / ops`. Every `ops.<name>` entry carries:
`op / samples / p50 / p95 / min / max / mean / DOM / RSS / verdict`.
`DOM` is `0` for timing-only ops and an object `{ naiveNodes, windowedNodes,
naiveRows, windowedRows, nodesPerRow, windowSize, r4Windowed }` for the DOM ops.

## Reproducibility

Fixtures come from a seeded PRNG (mulberry32, seed `20260808`), so every run
sees the identical dataset. Only compare runs on the **same machine + build**
(R-P1); machine fingerprint (`hostname/platform/arch/cpu/totalmem`) is recorded
in `baseline.json`.
