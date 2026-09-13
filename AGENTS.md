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

- Current production code is limited to the Chrome/Chromium extension,
  `crates/native-file-host`, `crates/file-manager-core`, and the single-purpose
  `model-host` authorized by ADR-0020. `src/`, `src-tauri/`,
  `src-agent-daemon/`, Agent/Harness/Capability crates, Jobs, Assistant, and
  Plugin Runtime have been deleted; do not recreate them.
- **Official managed apps exception (ADR-0027, 2026-09-09; converged 2026-09-12 to
  built-in modules in one complete product; supersedes the ADR-0026 Apps
  decisions)**: Natives is the single user-facing product. Fund and other
  official features are **built-in modules** delivered inside the complete
  Natives installer — no module download, install, update, uninstall, Catalog
  release, or fund `.nap`/Release outside the product. Internal modules
  (portfolio, ledger, nav, import, migration) belong to the package and do not
  have separate install records or product identities. Module code is built
  with Natives and enters the full product package; installation, updates, and
  repairs happen only at product level ("update Natives"). The Core App Store
  (`crates/native-file-host`) verifies, registers, and signs checks at product
  install/update time; active app payloads and data stay within
  `~/.natives/apps/<appId>/`, while minimal Host manifests use the
  browser-prescribed registration directories; writing to `/Applications`,
  independent `.app` bundles, Dock, or LaunchServices is strictly forbidden.
  The generic `extension/app.html` is each package's owner page — a short Core
  verification connection, then a direct Native Port to the app host, with the
  app UI in a restricted sandbox iframe over the app's 127.0.0.1 loopback
  server. The shared runtime library is `crates/app-host-support`. The App
  Center only offers open/show-hide/preferences/data management for built-in
  modules; adding modules requires a new complete Natives version. First use
  does data initialization only — no code download, no "installing fund".
  This exception does NOT authorize third-party web URLs, third-party native
  packages, Agent/Harness/Jobs, generic Plugin Runtime, Service Worker
  ports/polling, or any fund-specific logic inside the extension/Core. Legacy
  dynamic-registration/module-distribution paths must not remain as silent
  fallbacks; until migration completes, do not keep two production chains.
- **Unified suite delivery (ADR-0029, 2026-09-11; converged 2026-09-12 to
  single-product built-in modules)**: the Natives installer is one complete
  product containing the thin launcher entry, the Chrome extension component,
  main Hosts, and all built-in modules (fund in the first complete candidate).
  First use of a module does data initialization only, as the current OS user;
  there is no offline seed/seed-reconciliation chain and no second download.
  The package installer never writes user DBs/activation or runs app
  migrations as root. A root-owned macOS system source under
  `/Library/Application Support/Natives/` may hold the thin launcher, main
  Hosts, and fixed built-in module files, not user data or a second App
  Registry. Do not restore the old Workbench.app. A sole thin Natives
  launcher (open Chrome, locate the extension directory, brief diagnostics,
  then exit) is the precise ADR-0020 exception. Reinstall/update/repair must
  preserve module display/hide preferences, disabled/removed choices, user
  data, and Keychain; implicit downgrades are rejected. Module business stays
  built with the product; internal modules are never separately installed.
- **Local and release gates**: A-Local must pass with the real built-in fund
  inside the complete product candidate before B0—B3; B-Local must pass before
  calling the local suite usable. Existing independent samples serve only as
  low-level test fixtures for shared logic. Local builds may use ad-hoc/development signatures only under the explicit,
  isolated non-production policy in the managed-app contract. Release rejects
  development keys/fixtures and still requires full A/B production evidence,
  platform signatures/notarization and separate publication authorization.
  Never call A-Local/B-Local full A-Gate/B-Gate, weaken sandbox/data controls,
  remove quarantine or disable Gatekeeper. Reuse the existing dev entry and
  single installation engine, not another production chain.
- The extension's `newtab.html` is static, while `files.html` directly owns its
  Native Messaging Port. The Host owns filesystem access and never exposes
  arbitrary paths, processes, SQLite, or Secret plaintext to the page.
- Service Worker code must remain stateless: no Native Port, polling, keepalive,
  or local service. Host cleanup is driven by Native Messaging stdin EOF.
- The extension page is the only product surface; iframes are only allowed for
  the ADR-0027 app sandbox exception (no `allow-same-origin`, no downloaded
  business JS in extension context); do not reintroduce WebView, React
  workspace, plugin, or runtime surfaces.
- `model-host` is an AI-domain Native Messaging Host, not a general daemon. It
  may remain resident only after the user explicitly enables that setting; it
  must bind loopback, remain single-instance, keep Secrets in OS Keychain, and
  must not own Files capabilities or expose CLIProxyAPI management endpoints.

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
