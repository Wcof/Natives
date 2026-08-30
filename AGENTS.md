# Natives agent instructions

## Source of truth

- Start with `docs/README.md`, then read `docs/standards/README.md` and the
  relevant 1–3 standards before editing code.
- Authority order: `docs/standards/` > ADR-0020 > other ADRs >
  `docs/architecture/` > root documentation.
- A change that relaxes a MUST requires an ADR first. Do not silently work
  around it.
- Use the task map in `docs/README.md`; do not duplicate architecture or
  progress snapshots in new documents.

## Target architecture and current migration

- Current production code is limited to the Chrome/Chromium extension plus
  `crates/native-file-host` and `crates/file-manager-core`. `src/`, `src-tauri/`,
  `src-agent-daemon/`, Agent/Harness/Capability crates, Jobs, Assistant, and
  Plugin Runtime have been deleted; do not recreate them.
- The extension's `newtab.html` is static, while `files.html` directly owns its
  Native Messaging Port. The Host owns filesystem access and never exposes
  arbitrary paths, processes, SQLite, or Secret plaintext to the page.
- Service Worker code must remain stateless: no Native Port, polling, keepalive,
  or local service. Host cleanup is driven by Native Messaging stdin EOF.
- The extension page is the only product surface; do not reintroduce iframe,
  WebView, React workspace, plugin, or runtime surfaces.

## Change discipline

- Use `rtk` for shell commands.
- Run Cargo from the repository root with the ambient target override removed:
  `rtk env -u CARGO_TARGET_DIR cargo ...`. The checked-in `.cargo/config.toml`
  then keeps every agent on the current checkout's single `target/`.
- Never create or select another Cargo target directory (`.cargo-target-*`, a
  worktree-local target, or a temporary target). Never run `cargo clean` on the
  shared target unless the user explicitly requests it.
- Do not run Cargo in secondary worktrees; run their Rust verification once
  from the main checkout after integration so artifacts are reused.
- Reuse existing domain modules, adapters, types, tokens, and test patterns
  before adding abstractions or dependencies.
- Keep user-visible data real, error states explicit, and extension locales in
  `extension/_locales/zh_CN` / `extension/_locales/en` synchronized.
- Preserve the applicable security defenses in
  `docs/standards/technical/02-security.md`; persistent Secret ownership is OS
  Keychain per ADR-0020/R-S12.
- Performance work MUST follow `docs/standards/technical/04-performance.md`
  and include comparable before/after evidence.
- Do not mix unrelated formatting, renames, generated output, or historical
  cleanup into a focused change.

## Rust lock and test-hang discipline

- Never call a method that can lock the same non-reentrant `Mutex` while its
  guard is still in scope. After a transaction commits, explicitly `drop` the
  connection guard before calling public snapshot/query methods, or use an
  existing helper that accepts the held connection.
- When fixing a lock bug, audit every caller and every sibling
  `commit -> snapshot/session/query` path; fix the shared pattern once rather
  than patching only the reported test.
- A Rust test that stops producing output is a suspected hang, not a slow
  build. Time-bound one exact test, inspect its stack, and check for stale
  matching Cargo/test processes before starting another run. Do not stack
  repeated attempts.
- Do not pipe an active test run to `tail`: `tail` cannot emit the final lines
  until the producer exits and therefore hides hangs. Run it directly while
  diagnosing; add output truncation only after the command is known to finish.

## Verification

During implementation, run only the smallest check that covers the changed
code. Do not repeat a passing check unless relevant code changed afterward.
For Rust, prefer an exact test or affected package before any workspace-wide
command. Separate compilation time from test execution when diagnosing a slow
test (`cargo test --no-run` first, then the exact test binary or test filter).

Run the applicable gates once, at final integration before merge/release:

```sh
rtk npm run extension:check
rtk npm run perf:check
```

Also run the final checks for the area touched:

- Rust: `rtk env -u CARGO_TARGET_DIR cargo fmt --check` and
  `rtk env -u CARGO_TARGET_DIR cargo test --workspace`.
- Extension: `rtk npm run extension:check` and `rtk npm run perf:files`.
