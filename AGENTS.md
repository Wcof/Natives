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

- `src/`: Next.js 15 static-export Renderer. Keep `app/**/page.tsx` thin;
  domain UI belongs in `components/`, reusable non-UI behavior in `lib/` or
  `hooks/`.
- `src-tauri/`: target default Native Backend for windows, PTY, host SQLite,
  Files, Apps, AI Resources, Local Proxy, AI Tool Integration, Usage, Keychain
  brokering, and supervised processes.
- `src-agent-daemon/`, Agent/Harness/Capability crates, Jobs, Assistant, and
  Plugin Runtime are migration-time legacy. Do not add features or new
  dependencies to them; only security, migration, parity, and deletion work is
  allowed.
- Current Provider execution still runs through `Renderer → Host → UDS →
  Agent Daemon`. This is a current-production fact, not the target. Cut over to
  Host only after ADR-0020 P0 parity/Secret/lifecycle gates, then remove the old
  caller and fallback in the same slice.
- Renderer never opens SQLite, performs heavy filesystem/process work, calls
  Provider upstreams, or handles Secret plaintext directly.
- Existing Host/Daemon wire types remain sourced from
  `crates/assistant-protocol` until deletion; generated frontend bindings under
  `src/types/generated/` are not hand-edited.
- Existing Workshop iframe and Embed WebView remain separate trust domains
  until legacy removal. Never weaken their sandbox/Bridge defenses early.
- Home is the only V1 workspace. Widgets are built-in React renderers + config
  + versioned grid layout, never plugins or a runtime.

## Change discipline

- Use `rtk` for shell commands.
- Run Cargo from the repository root with the ambient target override removed:
  `rtk env -u CARGO_TARGET_DIR cargo ...`. The checked-in `.cargo/config.toml`
  then keeps every agent on the current checkout's single `target/`.
- Never create or select another Cargo target directory (`src-tauri/target`,
  `.cargo-target-*`, a worktree-local target, or a temporary target). Never run
  `cargo clean` on the shared target unless the user explicitly requests it.
- Do not run Cargo in secondary worktrees; run their Rust verification once
  from the main checkout after integration so artifacts are reused.
- Reuse existing domain modules, adapters, types, tokens, and test patterns
  before adding abstractions or dependencies.
- Keep user-visible data real, error states explicit, and `src/i18n/zh.ts` /
  `src/i18n/en.ts` synchronized.
- Preserve the applicable security defenses in
  `docs/standards/technical/02-security.md`; persistent Secret ownership is OS
  Keychain per ADR-0020/R-S12.
- Performance work MUST follow `docs/standards/technical/04-performance.md`
  and include comparable before/after evidence.
- Do not mix unrelated formatting, renames, generated output, or historical
  cleanup into a focused change.

## Verification

During implementation, run only the smallest check that covers the changed
code. Do not repeat a passing check unless relevant code changed afterward.
For Rust, prefer an exact test or affected package before any workspace-wide
command.

Run the full applicable gates once, at final integration before merge/release
(not once per agent or intermediate handoff):

```sh
rtk npm run typecheck
rtk npm run lint
rtk npm run test
rtk npm run perf:check
```

Also run the final checks for the area touched:

- Rust: `rtk env -u CARGO_TARGET_DIR cargo fmt --check` and
  `rtk env -u CARGO_TARGET_DIR cargo test --workspace`.
- Host/Daemon protocol: `rtk npm run protocol:check`.
- Native engine: `rtk npm run verify:native-engine`.
- Extension host: `rtk npm --prefix extension-host run typecheck` and
  `rtk npm --prefix extension-host run test`.
