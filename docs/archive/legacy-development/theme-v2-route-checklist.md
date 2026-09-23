# Theme V2 Route & Component Migration Checklist (V-053)

> Design System V2: Dark Glow / Liquid Crystal
> Status: **MIGRATED & FROZEN** (2026-08-21)

## 1. Page Routes Matrix (11 Routes)

| Route | Primary Component | Dark Glow Status | Liquid Crystal Status | Sign-off |
|---|---|---|---|---|
| `/` | `src/components/workspace/WorkspaceCompositionPage.tsx` | PASS (4-level surfaces, subtle glow) | PASS (specular highlight, micro-shadows, watercolor charts) | ✅ Complete |
| `/files` | `src/components/files/FileBrowser.tsx` | PASS (stable readable surface) | PASS (crystal preview panes, graphite text) | ✅ Complete |
| `/apps` | `src/components/apps/AppsPage.tsx` | PASS (low-contrast cards) | PASS (crystal cards, layered elevation) | ✅ Complete |
| `/ai` | `src/components/ai/AiWorkbench.tsx` | PASS (external tool UI, no agent runtime) | PASS (crystal panels, graphite labels) | ✅ Complete |
| `/usage` | `src/components/dashboard/UsageDashboard.tsx` | PASS (luminous lines, 8% fill) | PASS (15% -> 0% watercolor area gradient) | ✅ Complete |
| `/capabilities` | `src/components/capabilities/CapabilitiesPage.tsx` | PASS (high-density cards, restrained status) | PASS (crisp contrast, micro-shadows) | ✅ Complete |
| `/library` | `src/components/library/LibraryPage.tsx` | PASS (clean surface, subtle border) | PASS (crystal surface, graphite text) | ✅ Complete |
| `/modules` | `src/app/modules/page.tsx` -> Workshop deep-link | PASS | PASS | ✅ Complete |
| `/store` | `src/app/store/page.tsx` -> Local apps deep-link | PASS | PASS | ✅ Complete |
| `/tools` | `src/components/tools/ToolsPage.tsx` | PASS | PASS | ✅ Complete |
| `/jobs` | `src/components/jobs/JobsPage.tsx` | PASS (background tasks UI) | PASS (background tasks UI) | ✅ Complete |

## 2. Shell & Overlays Matrix

| Area | Component | Dark Glow | Liquid Crystal | Sign-off |
|---|---|---|---|---|
| Shell Root | `ShellLayout.tsx` / `shell.css` | PASS | PASS (clean canvas, spec highlight) | ✅ Complete |
| Navigation | `Sidebar.tsx` / `parts.tsx` | PASS (collapsed 64px rail, active glow) | PASS (crystal sidebar, graphite text) | ✅ Complete |
| Overlays | `CommandPalette.tsx` | PASS (floating elevation) | PASS (diffuse micro-shadows) | ✅ Complete |
| Notifications | `NotificationPanel.tsx` | PASS | PASS | ✅ Complete |
| Terminal | `Terminal.tsx` / `terminal.css` | PASS (ANSI contrast) | PASS (clean terminal surface) | ✅ Complete |
| Settings | `SettingsPage.tsx` | PASS (theme picker with V2 labels) | PASS (theme picker with V2 labels) | ✅ Complete |
| Modals | `Modal.tsx`, `ConfirmDialog.tsx` | PASS (backdrop-filter restrained) | PASS (crystal modal, multi-shadows) | ✅ Complete |
| Feedback | `Toast.tsx`, `Skeleton.tsx`, `Empty.tsx`, `Error.tsx` | PASS | PASS | ✅ Complete |

## 3. Mandatory Liquid Crystal Rules Compliance

1. **Specular Highlight**: `var(--specular-highlight)` applied to cards, popups, and top edges.
2. **Multi-layer Micro-shadows**: 2–3 layers of diffuse low-alpha shadows replacing thick solid borders.
3. **Graphite/Neutral Gray Typography**: Primary text graphite `#1a1c20`, secondary `#5a606b`, tertiary `#8a909b`.
4. **Chart Area Fill 15% -> 0%**: `ChartAreaGradient` helper enforces translucent single-hue gradients.
5. **No mechanical inversion**: Light theme is an independently designed frost/crystal system.
