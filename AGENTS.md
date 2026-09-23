# Natives agent instructions

## Source of truth

- Start with `docs/README.md`, then read `docs/standards/README.md` and the
  relevant 1–3 standards before editing code.
- Authority order: ADR-0031 (current highest architecture authority) >
  `docs/standards/` > other ADRs (historical context only) >
  `docs/architecture/` > root documentation. Historical ADRs explain history;
  never use them as an execution entry to restore deleted production paths.
- A change that relaxes a MUST requires an ADR first. Do not silently work
  around it.
- Use the task map in `docs/README.md`; do not duplicate architecture or
  progress snapshots in new documents.

## Target architecture and current migration

- Current production code is limited to the Chrome/Chromium extension,
  `crates/native-file-host`, `crates/file-manager-core`, the single-purpose
  `model-host` authorized by ADR-0020, and the unified product app runtime
  `crates/app-runtime` / `crates/app-runtime-core` (producing `natives-app-runtime`
  authorized by ADR-0031) with official built-in modules in `modules/` (such as
  `modules/fund`). `src/`, `src-tauri/`, `src-agent-daemon/`, Agent/Harness/Capability
  crates, Jobs, Assistant, and Plugin Runtime have been deleted; do not recreate them.
- **Unified built-in app runtime and monorepo modules (ADR-0031, 2026-09-14; supersedes
  per-app executable and per-app Native Host decisions in ADR-0027/0029)**: Natives is
  the single user-facing product. Fund and other official features are **built-in modules**
  whose source code lives in `modules/<appId>` within the Natives Monorepo and compiles
  statically into the single product-level `natives-app-runtime` binary. There is
  **no per-app executable (no fund-host)** and **no per-app Native Messaging Host registration
  (no com.natives.app.a<hash>)**; the OS Native Messaging manifest registers solely
  `com.natives.app_runtime` (or `com.natives.local.app_runtime` for local development).
  When a user opens an app in `app.html?app=<id>`, Chrome launches an independent, on-demand
  `natives-app-runtime` process instance (Single Runtime Binary / Multi Process Instance).
  Unselected modules consume zero dedicated memory (≈0 dedicated runtime memory). When the
  port disconnects (stdin EOF) or the page closes, the runtime process exits completely in
  $\le 2$ seconds.
  Internal modules (portfolio, ledger, nav, import, storage, migration) belong to the module
  and do not have separate install records, card projections, or product identities.
  The Core App Store (`crates/native-file-host`) registers and verifies product manifest v2
  at product install/update time. User application roots (`~/.natives/apps/<appId>/`) strictly
  contain `activation.json`, `data/` (`fund.db`), `imports/`, `cache/`, `logs/` — no executables
  are placed in user directories.
  The generic `extension/app.html` connects to `com.natives.app_runtime`, selects the module
  via App Runtime Protocol v2 (`app:handshake`), and hosts the app UI in a restricted sandbox
  iframe over loopback 127.0.0.1 (runtime defaults to `127.0.0.1:8765`, falls back to a dynamic
  port when occupied — ADR-0032, registry in `docs/standards/technical/05-port-registry.md`).
  Modules must not write to `/Applications` or create
  independent `.app`, Dock, or LaunchServices entries.
- **Unified suite delivery (ADR-0029, 2026-09-11; converged 2026-09-12 to
  single-product built-in modules)**: the Natives installer is one complete
  product containing the thin launcher entry, the Chrome extension component,
  main Hosts, and all built-in modules (fund in the first complete candidate).
  First use of a module does data initialization only, as the current OS user;
  there is no offline seed/seed-reconciliation chain and no second download.
  The package installer never writes user DBs/activation or runs app
  migrations as root. A root-owned macOS system source under
  `/Library/Application Support/Natives/` holds main Hosts, the fixed unpacked
  Chrome extension, and built-in module files, not user data or a second App
  Registry. **Dev product-source revision (2026-09-17, ADR-0029):** the
  root-owned source is a release/installer-candidate requirement only. Everyday
  local iteration (module UI changes, rebuilding `natives-app-runtime`, payload
  SHA re-sealing, dev manifest re-signing) must stay in the user-writable
  `~/.natives-local/` namespace (product source defaults to
  `~/.natives-local/product-source/`, overridable via environment variable)
  and must not require sudo; dev tooling must not make sudo a prerequisite of
  routine iteration. Build the root-owned source only for installer-candidate
  verification (A-Local/B-Local installer acceptance). Verification rules
  (signature, payload hash/摘要) are unchanged in either layout.
  **Visible launcher revision (2026-09-13, explicit user decision):**
  the same installer must provide `/Applications/Natives.app` with a normal
  name, icon, and system application registration. Double-click opens Chrome;
  first use shows extension loading guidance and locates the bundled folder;
  an actual current-launch handshake hands off to the extension. This sole
  main-product `.app` and its brief native setup/diagnostic window are the
  precise ADR-0020/0029 exception. No module `.app`, Workbench, desktop business
  UI, forced Dock pinning, persistent launcher, or second install engine.
  Local development uses an isolated launcher name/id/path. Do not claim
  silent local extension installation in ordinary Chrome; show the required
  browser steps when automatic installation is unavailable. The launcher
  bundles an offline HTML guide that opens in Chrome before extension setup;
  the installer conclusion shows a static summary generated from the same
  content. The guide needs no installed extension, Native Bridge, business
  data, or local web server; it cannot report authenticated connection state.
  The launcher exits after handoff or cancellation. Reinstall/update/repair must
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
- The extension page is the only business surface; the main launcher may show
  only the setup/diagnostic window authorized above. Iframes are only allowed for
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
