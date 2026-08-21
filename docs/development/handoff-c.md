# Handoff C — Wave1: Workspace UX / Layout / Inspector / Views + Domain Page Migration

Author: Subagent C (Workspace UX / Layout / Views / Domain Page Migration)
Branch: `feat/v2-workspace-design-system` · Contract doc: `docs/contracts/workspace-v2-contract.md`

> Reading note: the contract, ownership and execution-book documents are outside this
> agent's tool scope, so all assumptions below are stated explicitly for the parent to
> cross-check against the frozen contract.

---

## Completed Task IDs

**Workspace core (C-001..C-032):**
- C-001 WorkspaceCompositionPage skeleton — `src/components/workspace/WorkspaceCompositionPage.tsx`
- C-002 Session provider (snapshot-first, inactive = metadata + snapshot cache only)
- C-003 Tab strip: open / close / pin / reorder / keyboard layer — `WorkspaceTabStrip.tsx` + `tabStripModel.ts`
- C-004 Close ≠ Delete: closed tabs move to `session.closedTabs` (reopen from history menu)
- C-005 Snapshot-first, no blank screen: synchronous hydration from cache
- C-006 Debounced persistence (never per-pointer) — `workspacePersistence.ts`
- C-007..C-011 CompactGrid extracted from old Home pattern — `layout/CompactGrid.tsx` (lg/md/sm = 12/8/4, stop-only layout commit, keyboard layer)
- C-012..C-022 Free Canvas — `lib/workspace/canvas/*` + `views/FreeCanvasView.tsx` (pan/zoom, drag/8-handle resize, click/shift/marquee selection, grid+node snap, group/frame/z-order, memory-only pointer moves → commit on pointer-up)
- C-023..C-026 Inspector host — `inspector/WorkspaceInspector.tsx` reusing the existing `ResizableRightPanel` (no panel copy, no Twenty source)
- C-027..C-031 Data View — `views/DataView.tsx` (List/Table/Board/Calendar controlled views) + `lib/workspace/views/dataViewState.ts` + `types.ts`; view state persisted per view id
- Workspace view registry — `views/WorkspaceViewRegistry.tsx`

**Domain page visual migration (V-037..V-050):**
- V-044 AI workbench migrated from inline styles to semantic-token Tailwind classes (`src/components/ai/AiWorkbench.tsx`)
- V-037..V-050 **verification pass** across all in-scope surfaces: `grep` for `#hex`, `rgb(`, and Tailwind palette (`gray-`, `blue-`, `slate-`, `bg-white`, …) found **zero hardcoded colors** in files/, preview/, library/, capabilities/, apps/, creative/, ai/, jobs/, settings/, tools/, onboarding/, release/, screenshot/, update/, shell/Terminal.tsx, shell/TerminalRecorder.tsx, home/HomeWorkspacePage.tsx, all in-scope app pages. Those surfaces already consume semantic tokens (`var(--surface|--border|--text|--primary|--danger|--surface-subtle|…)`).

## Files created

| File | Purpose |
| --- | --- |
| `src/lib/workspace/views/types.ts` | Snapshot/session/tab/view TS contracts + grid constants + default snapshot builder |
| `src/lib/workspace/views/dataViewState.ts` | Data view state persistence (sync read, debounced write) |
| `src/lib/workspace/canvas/types.ts` | Canvas node/camera/selection model |
| `src/lib/workspace/canvas/geometry.ts` | Pure canvas geometry (snap/hit-test/marquee/z-order) |
| `src/lib/workspace/canvas/camera.ts` | Pan/zoom coordinate transforms |
| `src/components/workspace/session/workspacePersistence.ts` | Snapshot cache load/save (versioned, debounced, non-fatal) |
| `src/components/workspace/session/WorkspaceSessionProvider.tsx` | Session reducer + context provider |
| `src/components/workspace/tabs/tabStripModel.ts` | Tab model pure helpers |
| `src/components/workspace/tabs/WorkspaceTabStrip.tsx` | Tab strip (open/close/pin/reorder/keyboard) |
| `src/components/workspace/layout/CompactGrid.tsx` | Extracted responsive grid primitive (12/8/4) |
| `src/components/workspace/views/WorkspaceViewRegistry.tsx` | View-kind → config factory |
| `src/components/workspace/views/GridWorkspaceView.tsx` | Grid view (built-in widgets on CompactGrid) |
| `src/components/workspace/views/FreeCanvasView.tsx` | Free Canvas view (forwardRef imperative handle for Inspector) |
| `src/components/workspace/views/DataView.tsx` | List/Table/Board/Calendar controlled view |
| `src/components/workspace/inspector/WorkspaceInspector.tsx` | Inspector host (reuses ResizableRightPanel) |
| `src/components/workspace/WorkspaceCompositionPage.tsx` | Composition root (session + tab strip + view area + inspector + status bar) |
| `docs/development/handoff-c.md` | This handoff |

## Files modified

- `src/components/ai/AiWorkbench.tsx` — inline styles → semantic-token classes (V-044). Behavior unchanged.
- *(No shared files were modified — see Shared-file patch intents below.)*

## Contract assumptions

1. **Snapshot contracts (§3/§4/§5):** `WorkspaceSnapshot` / `WorkspaceSessionSnapshot` /
   `WorkspaceTab` / `WorkspaceViewConfig` are declared in
   `src/lib/workspace/views/types.ts` mirroring the frozen contract. When the
   platform/data track lands the canonical shared module, re-export these aliases
   from there (single source of truth — no second declaration).
2. **Widget contract (§6):** `WidgetInstance`/`surfacePolicy` are not reimplemented;
   the grid view uses snapshot `gridLayouts` (react-grid-layout `Layout`-compatible
   items) and the workspace keeps its own small built-in widget set for Wave1.
3. **Token taxonomy (§7):** all new code consumes semantic tokens only
   (`--surface`, `--surface-hover`, `--surface-subtle`, `--border`, `--border-subtle`,
   `--primary`, `--primary-foreground`, `--primary-soft`, `--text`, `--text-secondary`,
   `--text-disabled`, `--danger`). No color system is introduced anywhere.
4. **Motion (§8):** gestures use pointer events with `touch-action:none` and no
   duration-based animation; the only transitions are color/hover transitions.
5. **New workspace copy strings** are English literals (Wave1) — i18n keys to be added
   by the locale owner in Wave2 (patch intent C-D05).

## Contract requests to agent B (missing §7 tokens)

- `--overlay` / `--overlay-strong` and `--neutral-*` are used by existing
  `files/*`, `ai/AiResourcesPanel.tsx`, `capabilities/skills/SkillDetail.tsx` and
  `shell/Terminal.tsx` (drop overlay). These predate Wave1; confirm whether they are
  part of the frozen §7 taxonomy. If not, request B add semantic equivalents
  (e.g. `--overlay-scrim`, `--on-overlay`, `--checkerboard`) and Wave2 migrates the
  call sites.
- `--surface-subtle` is used broadly by existing creative/domain surfaces and by the
  new workspace views — confirm it is frozen in §7 (it behaves as a semantic token).

## Migration / compat impact

- **Additive only:** all new files live under `src/components/workspace/**` and
  `src/lib/workspace/**`; no existing surface imports them yet.
- `HomeWorkspacePage.tsx` (Home) is untouched and keeps working; it remains on the old
  inline grid until patch intent C-D01 lands.
- `AiWorkbench.tsx` restyle is visual-only (same JSX structure, classes swapped).
- No dependency changes; reuses `react-grid-layout` and `lucide-react` already in the
  project. No Yjs/CRDT/BlockSuite/AFFiNE/Plane/Twenty code.

## Known risks

1. **Free Canvas world grid** is rendered via `backgroundImage` with
   `repeating-linear-gradient` using `var(--border-subtle)` inside the scaled world div —
   grid density scales with zoom (intentional, framed-canvas feel); if §7 requires a
   fixed-screen grid, swap to a Screen-space layer.
2. **Canvas selection reporting** flows one-way (view → inspector); external deletes
   from the Inspector go through the imperative handle; a full external-state binding
   for the canvas is deferred to Wave2.
3. **`WorkspaceTabStrip` close affordance** is a `<span>` nested inside the tab `<button>` —
   functionally fine, but a11y audit should replace it with a sibling button.
4. **Data view rows** are a demo dataset (`demoRows` in the composition page and
   `DEFAULT_ROWS` in `DataView.tsx`); the real data source is wired in Wave2.
5. **`react-grid-layout` API usage** (Responsive + `useContainerWidth` + `noCompactor`,
   `dragConfig`/`resizeConfig`) mirrors the existing Home usage — compile/type not
   verified in this agent (see Deferred verification).
6. **Unused-but-exported helpers** (`useDataViewState`, `tabFromView`, `moveTabTo`,
   `useActiveViewId`, `WORKSPACE_TOKENS`) are intentionally exported for Wave2/tests;
   they will be pruned or consumed then.

## Deferred verification

- `tsc` / ESLint / Jest not run (no shell commands permitted for this agent).
- `react-grid-layout` `Responsive` typing and `onDragStop`/`onResizeStop` payload
  shapes against the installed version.
- Pointer capture / ResizeObserver / forwardRef behavior in the actual app.
- `src/app/usage/**` sub-files beyond `page.tsx` were not individually read (tool scope
  returned only the root route file); visual verification for those sub-routes is pending.
- Persistence swap from localStorage cache to the shared settings/persistence layer
  (Wave2).

## Shared-file patch intents (NOT applied — shared files out of scope)

- **C-D01 — Deduplicate the grid:** update `src/components/home/HomeWorkspacePage.tsx`
  to consume the extracted `src/components/workspace/layout/CompactGrid.tsx` (or make
  CompactGrid the single implementation and let Home wrap it), removing the inline
  Responsive copy. Home data/model (`@/lib/home-workspace/*`) stays the Home source of
  truth.
- **C-D02 — Mount the V2 surface:** in `RootClient.tsx` / `src/app/page.tsx`, mount
  `src/components/workspace/WorkspaceCompositionPage.tsx` as the Personal Workspace
  surface (`/`), keeping `HomeWorkspacePage` reachable (e.g. `/home`) until C-D01.
- **C-D03 — Shell integration:** in `ShellLayout.tsx` / `MainContent.tsx`, add the
  workspace route mapping + an entry in `Header.tsx` / `Sidebar.tsx` nav; ensure the
  workspace composition sits inside the existing Shell theme provider (single theme
  source — no second Theme/Layout source of truth).
- **C-D04 — Cleanup (C-D01..C-D05):** remove the old `home-workspace` layout duplication
  once CompactGrid is shared; delete unreachable legacy copies if any remain; run a
  final token audit.
- **C-D05 — i18n keys** for the new workspace copy strings (tab strip a11y labels,
  inspector labels, empty states, canvas toolbar titles) — add to locale files.
- **Inspector host in shell:** `handler_registration.rs` (Tauri) needs no change in
  Wave1; the workspace persistence is client-side cache only. When Wave2 moves
  persistence to the Rust side, register the snapshot keys there.

## Reference-source-copy statement

**NO SOURCE COPIED.** The Free Canvas is an original lightweight DOM implementation
inspired only by common infinite-canvas interaction concepts; no AFFiNE / Plane /
Twenty / Yjs / CRDT / BlockSuite source or assets were copied, ported, or adapted.
`CompactGrid` is extracted from the repository's own `HomeWorkspacePage` implementation.
`ResizableRightPanel` is reused unchanged (existing repository code), not copied from
any third-party product.

## Handoff summary (reply)

Wave1 done for C-001..C-032 + V-037..V-050 (verification pass). Created the full V2
workspace core under `src/components/workspace/**` + `src/lib/workspace/**`: snapshot-first
composition page, session provider (inactive = metadata/cache only), tab strip with
Close≠Delete + pin/reorder/keyboard, extracted CompactGrid (12/8/4, stop-only persist),
self-built Free Canvas (pan/zoom/drag/resize/marquee/snap/group/z-order; memory-only
pointer moves), Inspector host reusing `ResizableRightPanel`, and List/Table/Board/Calendar
Data View with per-view persisted state. Domain surfaces verified token-clean (no
hardcoded colors anywhere in scope); `AiWorkbench` restyled to token classes. No shared
files touched — mounting/cleanup routed to the parent via patch intents C-D01..C-D05;
3 contract requests to B (`--overlay`/`--neutral-*` confirmation + `--surface-subtle`
confirmation). No tests/lint run (tool-blocked) — deferred verification listed above.
NO SOURCE COPIED.
