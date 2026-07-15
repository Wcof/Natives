# Usage Ledger Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace dashboard scan-only statistics with a local, deduplicated usage ledger while preserving ten metrics, manual sync, dismissible warnings, and complete bilingual UI.

**Architecture:** Claude and Codex adapters normalize source records into one SQLite ledger keyed by source and event ID. Dashboard aggregation reads the ledger; the existing snapshot remains a read cache. New tools join by adding an adapter, not a dashboard code path.

**Tech Stack:** Rust, rusqlite, Tauri, React, TypeScript, existing i18n and CSS modules.

---

### Task 1: Add idempotent ledger storage

**Files:**
- Create: `src-tauri/src/usage/ledger.rs`
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/usage/mod.rs`

- [ ] **Step 1: Write ledger storage tests**

```rust
#[test]
fn upsert_replaces_an_event_without_changing_the_event_count() {
    let conn = Connection::open_in_memory().unwrap();
    create_usage_ledger_tables(&conn).unwrap();
    upsert_usage_events(&conn, &[event("claude", "request-1", 10)]).unwrap();
    upsert_usage_events(&conn, &[event("claude", "request-1", 25)]).unwrap();
    assert_eq!(usage_event_count(&conn).unwrap(), 1);
    assert_eq!(usage_event_tokens(&conn, "claude", "request-1").unwrap(), 25);
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml upsert_replaces_an_event_without_changing_the_event_count`

- [ ] **Step 3: Implement the smallest ledger**

Create `UsageLedgerEvent` with source, stable event key, session, timestamp, dimensions, token values and message counters. Add `usage_ledger_events` with `UNIQUE(source_id, event_key)`, then use one SQLite upsert statement. Expose only `upsert_usage_events` and range queries from `ledger.rs`.

- [ ] **Step 4: Run the test and verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml upsert_replaces_an_event_without_changing_the_event_count`

### Task 2: Normalize Claude and Codex records into the ledger

**Files:**
- Modify: `src-tauri/src/usage/claude.rs`
- Modify: `src-tauri/src/usage/codex.rs`
- Modify: `src-tauri/src/usage/aggregate.rs`
- Test: parser unit tests in `claude.rs` and `codex.rs`

- [ ] **Step 1: Add failing conversion tests**

```rust
assert_eq!(claude_event.event_key, "message-id:request-id");
assert_eq!(codex_event.event_key, "event-id");
assert_eq!(codex_event.total_tokens, 120);
```

- [ ] **Step 2: Run parser tests and verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage::claude::tests usage::codex::tests`

- [ ] **Step 3: Convert existing parsed records without duplicate parsers**

Map the existing `ParsedEvent` and `ParsedCodexEvent` structures to `UsageLedgerEvent`. Preserve current Claude request de-duplication, Codex delta calculation, and replay/fork exclusion. During manual sync write both collections to the ledger before building the snapshot.

- [ ] **Step 4: Run parser tests and verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage::claude::tests usage::codex::tests`

### Task 3: Aggregate dashboard data from the ledger

**Files:**
- Modify: `src-tauri/src/usage/ledger.rs`
- Modify: `src-tauri/src/usage/aggregate.rs`
- Test: `src-tauri/src/usage/ledger.rs`

- [ ] **Step 1: Add a range aggregation test**

```rust
let dashboard = query_ledger_dashboard(&conn, start_ms, end_ms, "Asia/Shanghai").unwrap();
assert_eq!(dashboard.daily[0].total_tokens, Some(120));
assert_eq!(dashboard.sessions.len(), 1);
```

- [ ] **Step 2: Implement daily, hourly and session queries**

Use SQLite `GROUP BY` for daily dimensions, hourly dimensions and source/session identity. Sum Token/message columns, calculate session bounds, and calculate active duration from sorted timestamps in Rust with the existing five-minute cap. Keep unavailable cost as `NULL`.

- [ ] **Step 3: Make snapshot sync consume ledger aggregates**

After adapters write the ledger, build the dashboard response from ledger aggregates. Keep ccusage as a distinct external source; do not merge its Claude/Codex rows into the same source IDs.

- [ ] **Step 4: Run focused tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage::ledger::tests`

### Task 4: Update dashboard behavior and localization

**Files:**
- Modify: `src/components/dashboard/UsageDashboard.tsx`
- Modify: `src/components/dashboard/UsageDashboard.module.css`
- Modify: `src/components/dashboard/UsageMetricGrid.tsx`
- Modify: `src/components/dashboard/UsageCharts.tsx`
- Modify: `src/i18n/zh.ts`
- Modify: `src/i18n/en.ts`

- [ ] **Step 1: Add a dismissible warning state**

Use `useEffect` to start a 10-second timer for a new snapshot warning fingerprint. Store dismissed fingerprints in component state, clear the timer on unmount, and render an accessible close button with the remaining seconds.

- [ ] **Step 2: Remove the title and localize all dashboard strings**

Delete the `Vibe Usage` heading. Replace card delta `New`, all tooltip labels, source labels and hardcoded warning strings with `t(locale, 'usage.*')` keys in both locale files.

- [ ] **Step 3: Run the frontend type check**

Run: `npm run typecheck`

### Task 5: Verify the complete change

**Files:**
- Modify: `docs/superpowers/specs/2026-07-15-usage-ledger-dashboard-design.md` only if the implementation differs from an approved decision.

- [ ] **Step 1: Run Rust checks**

Run: `cargo test --manifest-path src-tauri/Cargo.toml usage::ledger::tests && cargo check --manifest-path src-tauri/Cargo.toml`

- [ ] **Step 2: Run frontend checks**

Run: `npm run typecheck && git diff --check`

- [ ] **Step 3: Review the diff**

Confirm that no external scan occurs in `usage_get_cached`, the warning is not permanent, and every new display string comes from both locale files.
