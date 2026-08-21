# HOME-P0-GRID-SPIKE Report

## Baseline

- AiNative commit: `1497cf7`
- Node: `22.23.2` (outside the repository's required `>=20.11.0 <21` range)
- npm: `10.9.8`
- React / React DOM: `19.2.7`
- Next: `15.5.19`
- macOS: `26.5.2` (`arm64`)
- Candidate: `react-grid-layout@2.2.4`
- Headed surface: Codex in-app browser; engine build was not exposed
- Tauri / WebKit: not exercised by this preliminary spike

## Matrix

| Case | Result | Evidence |
|---|---|---|
| 5 widgets | PASS | Headed DOM count 5; no overlap/offscreen/runtime error |
| 20 widgets | PASS | Headed DOM count 20; no overlap/offscreen/runtime error |
| 40 widgets | PASS | Headed DOM count 40; no overlap/offscreen/runtime error |
| `lg` / `md` / `sm` | PASS | Headed widths selected `lg` at 1032px, `md` at 776px, `sm` at 437px |
| 1440 expanded/collapsed | PASS | 1192px / 1376px, both `lg` |
| 1280 expanded/collapsed | PASS | 1017-1032px / 1216px, both `lg` |
| 1024 expanded/collapsed | PASS | 776px / 960px, both `md` |
| normal-mode drag | PASS | Pointer drag left geometry unchanged; stop commits remained 0 |
| edit-mode drag | PASS | Pointer drag changed transform from `(24,24)` to `(273,244)`; commit occurred on stop |
| edit-mode resize | PASS | 237x164px became 320x252px; commit occurred on stop |
| post-interaction geometry | PASS | No overlap and no horizontal offscreen item |
| SSR / hydration | PASS (browser stage) | Static prerender succeeded; headed console had zero warnings/errors |
| packaged Tauri | PENDING | Required before H0-014 or production adoption |

The pure tests also cover all 5/20/40 layouts at every breakpoint, invalid
position/size clamping, non-mutation, and deterministic repeated restore.

## Metrics And Limits

- The fixture's bounded `PerformanceObserver` recorded 0 long tasks during the
  sampled headed interactions. This is a smoke signal, not release evidence.
- The final Next build completed with one repository-tooling warning: the root
  flat ESLint config does not register the Next plugin. Explicit spike source
  lint passed; no hydration or headed browser runtime warning was observed.
- The observer and window error listeners are disconnected on unmount.
- No `onDrag` or `onResize` handler is wired. Layout state is committed only
  from `onDragStop` and `onResizeStop`; there is no persistence or IPC in the
  spike.
- FPS, RSS, WebKit observer behavior, Retina/non-Retina hardware, zoom
  90/100/110%, 100 drag cycles, 100 resize cycles, 200 viewport resize cycles,
  100 sidebar toggles, and long-run listener/timer baselines remain unmeasured.
- Development HMR is excluded from performance conclusions.

## Recheck（2026-08-20，deploy 工作树）

- `spike:home-grid:test`：4 pass（layoutModel 归一化/钳制/重叠/确定性恢复）。
- `spike:home-grid:typecheck`：通过。
- `spike:home-grid:build`：通过（仅仓库级 ESLint 未注册 Next plugin 的既有告警）。
- 组件证据复核：无 IPC invoke、无 SQLite、无业务 Timer（仅 `PerformanceObserver` + error listener，unmount 时 `disconnect/removeEventListener`）；布局状态只在 `onDragStop`/`onResizeStop` 提交，未接 `onDrag`/`onResize`，符合 ADR-0020「拖动/缩放过程不写 DB，stop 才持久化」。
- packaged Tauri/WebKit、Retina/zoom、100/200 循环与 RSS soak 仍属 H0-014 / Release Gate（ADR-0020 P0 Gate），不在此 spike 范围内。

## Outcome

The React 19 + Next 15 type/build and headed in-app-browser compatibility stage
passes. Packaged Tauri/WebKit headed behavior and release performance remain
unproven. This report does not mark H0-014 complete and does not authorize
production Home integration.
