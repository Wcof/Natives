# Handoff A — Workspace Host / Persistence / Rust Death (Wave1)

**Branch:** feat/v2-workspace-design-system
**Author:** Subagent A (Workspace Host / Persistence / Rust Death)
**Status:** Implementation complete; compilation & tests deferred to parent (environment forbids running cargo/npm/tests).

---

## Completed Task IDs

Wave1 scope A-001..A-031 + V-001..V-004 (as grouped below; IDs per the V2 task book — see Contract assumptions for where the frozen doc could not be read in this sandbox):

- **A-001..A-004** — DB v27 seven tables (`workspaces`, `workspace_tabs`, `workspace_context_items`, `workspace_widgets`, `workspace_layouts`, `workspace_view_states`, `workspace_tool_profiles`) + schema version bump + registry wiring.
- **A-005..A-012** — Rust module layering `src-tauri/src/workspace/**` (types → store → snapshot), opaque `ref_id` refs (no secrets), camelCase serde contract.
- **A-013..A-021** — Tauri command surface `commands/workspace.rs` (workspace CRUD + active, tabs, context items, widgets, layouts, view states, tool profiles, sessions), `db-state-changed` channel `workspace` emits.
- **A-022..A-028** — Frontend TS contract `src/lib/workspace/**` (`contracts.ts`, `client.ts`, `events.ts`, `session-store.ts`, `snapshot-store.ts`).
- **A-029..A-031** — Legacy Home compatibility: `settings:home_workspace` (schemaVersion 1) imported into `home` workspace at `migrate_v27`; `src/lib/home-workspace/model.ts` + `persistence.ts` rerouted through the typed IPC client (no second data authority).
- **V-001** — Theme vocabulary normalized to `dark` | `light` in `get_theme` (never returns legacy aliases across IPC).
- **V-002** — `settings:theme` normalization: `set_theme` persists/emits only canonical values; `migrate_v27` rewrites legacy aliases stored in DB.
- **V-004** — Backend legacy alias writes cleaned: `set_theme`/`builtin_tool_ghostty_sync_theme` write normalized values; `ghostty_config::normalize_theme_id` absorbs any remaining legacy caller input; `DEFAULT_THEME` is now `dark`.

## Files created

| File | Purpose |
| --- | --- |
| `src-tauri/src/workspace/mod.rs` | Workspace domain root: layering + curated re-exports. |
| `src-tauri/src/workspace/types.rs` | serde DTOs (camelCase) + command inputs; `normalize_theme`. |
| `src-tauri/src/workspace/store.rs` | Raw SQL CRUD over the seven v27 tables. |
| `src-tauri/src/workspace/snapshot.rs` | `WorkspaceSnapshot` / `WorkspaceSessionSnapshot` assembly. |
| `src-tauri/src/commands/workspace.rs` | Tauri command surface (typed IPC) + `db-state-changed` emits. |
| `src/lib/workspace/contracts.ts` | Frozen TS contract mirror of the Rust DTOs. |
| `src/lib/workspace/client.ts` | Typed IPC client (bridge resolver: `nativesAPI.workspace` → Tauri internals). |
| `src/lib/workspace/events.ts` | Event names/payloads + `onWorkspaceChanged` subscription helper. |
| `src/lib/workspace/session-store.ts` | Reactive store for the current `WorkspaceSessionSnapshot`. |
| `src/lib/workspace/snapshot-store.ts` | Per-workspace `WorkspaceSnapshot` cache + invalidation. |
| `docs/development/handoff-a.md` | This handoff. |

## Files modified

| File | Change |
| --- | --- |
| `src-tauri/src/db/migrations_steps.rs` | Added `migrate_v27` (seven tables + `settings:home_workspace` import + `settings:theme` normalization) and helpers `import_legacy_home_workspace`, `normalize_legacy_theme`; added `rusqlite::OptionalExtension` import. |
| `src-tauri/src/db/db_migrations.rs` | Registered `migrate_v27` (v26→v27) in the ordered registry. |
| `src-tauri/src/db.rs` | `SCHEMA_VERSION` `"26"` → `"27"`. |
| `src-tauri/src/commands/theme.rs` | V-002/V-004: `DEFAULT_THEME = "dark"`; `get_theme`/`set_theme`/`builtin_tool_ghostty_sync_theme` normalize to `dark`/`light`. |
| `src-tauri/src/ghostty_config.rs` | Added `pub normalize_theme_id`; `theme_to_ghostty` normalizes first (keeps legacy-alias tolerance for external callers). |
| `src/lib/home-workspace/model.ts` | Deprecation banner pointing to `workspace/contracts.ts`; types preserved. |
| `src/lib/home-workspace/persistence.ts` | Rewritten compat layer: load via `home` workspace snapshot, save via widget/layout typed IPC; legacy K/V is read-only fallback (browser dev). |

## Contract assumptions

The frozen contract docs (`docs/contracts/workspace-v2-contract.md`, `docs/contracts/file-ownership.md`, `docs/development/execution-books/A.md`) were **not readable from this sandbox** (read tools enforce the write-scope allowlist). I reconstructed the contract from the task brief + in-scope sources. These are assumptions the parent must confirm against the frozen doc:

1. **v27 table/column shapes** — seven tables as in `migrate_v27`; snake_case SQL columns, camelCase JSON over IPC. If the frozen contract pins different column names (e.g. `widget_type` vs `type`, `is_active` vs `active`), only `migrate_v27` + `store.rs` column lists + `types.rs` serde names need syncing.
2. **`WorkspaceSnapshot` / `WorkspaceSessionSnapshot` shapes** — `WorkspaceSnapshot` = workspace summary + tabs + contextItems + widgets + layouts + viewStates + toolProfiles; `WorkspaceSessionSnapshot` = workspaceId + activeTabId + same collections. If the contract defines sessions differently (e.g. a real session table), the session commands are the only affected surface.
3. **Command names** — `workspace_list/get/create/update/delete/set_active/snapshot`, `workspace_tab_*`, `workspace_context_*`, `workspace_widget_*`, `workspace_layout_save`, `workspace_view_state_save`, `workspace_tool_profile_bind/unbind`, `workspace_session_open/close/snapshot`. The bridge/command registry must bind these names.
4. **Event contract** — Host emits `db-state-changed` with channel `workspace` (mirroring `theme`); `events.ts` also tolerates a dedicated `workspace` event name. If the real event name differs, only `events.ts` constants change.
5. **Legacy Home mapping** — legacy document imports to a fixed `home` workspace id (kind `home`, active). UI agents may rely on `home` id for the migrated Home.
6. **Theme vocabulary** — canonical values only `dark` / `light`; `DEFAULT_THEME = "dark"` (was `terminal-volt`, which mapped to dark).

## Migration / compat impact

- **v26 → v27** is additive + idempotent. Fresh DBs: seven tables created, no legacy data. Existing DBs: `settings:home_workspace` (if present) imported once into `home` workspace + `workspace_widgets`/`workspace_layouts` rows, then the legacy key is deleted (no second authority). Corrupt legacy JSON is dropped, never fatal.
- **`settings:theme`** legacy values are rewritten to `dark`/`light` in the same migration; runtime `set_theme` writes normalized values only.
- **Ghostty config** still accepts legacy ids for any caller, but the canonical write path passes normalized ids; config file name is `config-<theme>.conf` with `dark`/`light` ids.
- **Frontend Home** — `loadHomeWorkspaceDocument`/`createDocumentSaver` signatures unchanged (UI drop-in), now backed by v27 via typed IPC; browser-dev falls back to legacy K/V read-only.

## Known risks

1. **Compile wiring is incomplete by design** — `lib.rs` (shared) must add `mod workspace;`, `commands/mod.rs` must add `pub mod workspace;`, and the handler registry must register the new commands; until then `crate::workspace` and the command fns are unreachable (see Shared-file patch intents).
2. **Dynamic SQL avoided; update_tab rewrote** to fixed-column UPDATE to dodge borrow/params issues — verified by eye only.
3. **`workspace_session_close` emits without a DB round-trip** — payload carries no `workspaceId`; if the contract requires one, add a parameter.
4. **Host-generated ids** (`ws_<nanos>_<seq>` style) are opaque but not UUID; fine for single-user local DB, not for multi-device sync.
5. **`update_workspace` clearing semantics** — empty string clears nullable columns (`icon`/`description`); `null` means "leave unchanged". If the contract prefers tri-state patches, adjust `WorkspaceUpdateRequest`.
6. **`db-state-changed` channel name assumption** — `events.ts` subscribes to both `workspace` and `db-state-changed`; confirm the helper's real emitted event name.

## Deferred verification

- `cargo check` / `cargo test` / `cargo clippy` for `src-tauri` (env forbids running commands). Watch: `store.rs` param/ToSql binding, `with_conn` closure lifetimes, `format!` const interpolation, `workspace/mod.rs` re-export collisions.
- `tsc`/Next build for `src/lib/workspace/**` and `src/lib/home-workspace/persistence.ts` (dynamic `@tauri-apps/api/event` import, `window.nativesAPI` typing).
- db tests asserting `SCHEMA_VERSION` / migration registry consistency (should now expect `27`).
- Exact contract field/command-name parity with the frozen doc (see assumptions 1–6).

## Shared-file patch intents

Files owned by other agents / shared — changes required, not performed here:

1. `src-tauri/src/lib.rs` (or wherever `mod`s are declared): add `mod workspace;` and ensure `pub(crate) use crate::workspace` paths resolve; wire `commands::workspace` module.
2. `src-tauri/src/commands/mod.rs`: add `pub mod workspace;` (or `mod workspace;` per repo convention).
3. Command registration / `handler_registration.rs` (or the Tauri `invoke_handler` list): register all `workspace_*` commands from `commands/workspace.rs` (names listed in Contract assumptions).
4. Frontend bridge (`src/lib/tauri` or the `nativesAPI` generator): bind the workspace command surface (either `nativesAPI.workspace.<command>` camelCase, or ensure `client.ts`'s `__TAURI_INTERNALS__.invoke` fallback works in the shipped build).
5. `ShellLayout`/Home shell (other agents): consume `src/lib/workspace/*` instead of writing `settings:home_workspace` directly.

## Reference-source-copy statement: NO SOURCE COPIED

No source code was copied from any reference project (Midday/Plane/AFFiNE/Twenty or any other external repo). All schema, DTOs, stores, and commands were written from the frozen-contract brief and the existing Natives codebase conventions. Inspired concepts (to declare, if any): the `db-state-changed` event channel pattern mirrors the existing `theme` command emit in this repo (in-repo, not external).
