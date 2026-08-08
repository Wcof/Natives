# T02 spike — internal row virtualizer vs @tanstack/react-virtual

- **Status**: spike (analysis only, no code change in this worktree).
- **Decision owner**: T01 Coordinator (dependency addition is Coordinator-exclusive).
- **Budgets referenced**: `docs/standards/technical/04-performance.md` R-P4
  (>200 UI items must not create full DOM), R-P7 (lazy-load heavy capabilities),
  R-P10 (bundle gate); `scripts/perf/check-bundle.mjs` 350 kB gzip per route.

## Context

The file browser must render unbounded directory listings without violating R-P4.
Two candidate ways to window the file list:

1. **Internal row virtualizer** — a fixed `WINDOW_ROWS = 200` row window rendered
   directly, with top/bottom spacers preserving scroll geometry. This is the model
   the T02 baseline harness measures (`dom_construct_*` ops: naive N-row DOM vs
   bounded 200-row window DOM).
2. **@tanstack/react-virtual v3** — the headless, measurement-based virtualizer
   (react bindings over `@tanstack/virtual-core`).

## Comparison

### Bundle

| approach | bytes | notes |
| --- | --- | --- |
| Internal row virtualizer | 0 kB | no dependency, no vendor surface |
| `@tanstack/react-virtual` v3 | ≈ 10–12 kB min / ≈ 4–5 kB gzip | plus `@tanstack/virtual-core` ≈ 6–7 kB min / ≈ 2–3 kB gzip; total ≈ 5–8 kB gzip on top of react bindings ≈ 1.5–2% of the 350 kB route budget |

Figures are published-package approximations (bundlephobia for v3.x); no install
was performed in this worktree (`npm install` is Coordinator-only). A real
`perf:bundle` before/after is required before any merge (R-P10).

### Behavior

- **Internal fixed-window**: deterministic and trivially testable for
  uniform-height rows. Gaps: no dynamic row-height measurement, no scroll
  restoration, no `scrollToIndex` / keyboard "jump to", no overscan tuning, no
  sticky headers, no horizontal mode.
- **@tanstack/react-virtual**: variable row heights via `measureElement`,
  overscan, `scrollToIndex`, horizontal lists, initial scroll offset, SSR mode,
  measurement cache. Heavier mental model (`itemKey`, `virtualItem` transforms,
  `getTotalSize`/`getVirtualItems` contract).

### Maintenance

- **Internal**: we own every edge case — window jumping, anchor drift on resize,
  selection during scroll, i18n/RTL — and all its tests. No upstream, no
  supply-chain or version churn.
- **@tanstack/react-virtual**: actively maintained, MIT, first-class TS types,
  wide community. Cost: a new runtime dependency to pin and update in an
  otherwise dependency-light renderer, plus keeping it out of the initial JS
  path (lazy-load on the files page only, R-P7).

## Recommendation for T01 Coordinator

**Do not introduce `@tanstack/react-virtual` at this stage.**

- File-browser rows are currently uniform height; the internal fixed-window row
  renderer satisfies R-P4 (the T02 baseline `dom_construct_5000` op shows
  `naive=40001` nodes vs `windowed=1603`) with zero bundle cost and full control
  over selection/anchor behavior. This matches the "reuse before dependencies"
  rule in `AGENTS.md`.
- Re-evaluate (with before/after perf **and** bundle measurements per R-P1/R-P10)
  when concrete requirements land that the internal windower cannot meet:
  variable-height detail rows, scroll restoration, programmatic `scrollToIndex`
  (e.g. search-results "jump to", keyboard nav), or a tree view.
- If T01 later approves the dependency: pin the exact version, run
  `npm run perf:bundle` before/after against the 350 kB gzip budget, and keep the
  import lazy (files page only) so the initial route JS is unchanged.
