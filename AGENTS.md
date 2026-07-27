# Natives agent instructions

## Source of truth

- Start with `docs/README.md`, then read `docs/standards/README.md` and the
  relevant 1–3 standards before editing code.
- Authority order: `docs/standards/` > product-freeze ADRs > other ADRs >
  `docs/architecture/` > root documentation.
- A change that relaxes a MUST requires an ADR first. Do not silently work
  around it.
- Use the task map in `docs/README.md`; do not duplicate architecture or
  progress snapshots in new documents.

## Current architecture

- `src/`: Next.js 15 static-export Renderer. Keep `app/**/page.tsx` thin;
  domain UI belongs in `components/`, reusable non-UI behavior in `lib/` or
  `hooks/`.
- `src-tauri/`: Tauri Host authority for windows, PTY, host SQLite, local
  files/modules, credential brokering, and Daemon supervision.
- `src-agent-daemon/` + `crates/`: Agent Daemon and shared Rust workspace;
  these own runs, providers, capability execution, events, and protocol logic.
- `extension-host/`: isolated TypeScript extension runtime with its own
  package scripts.
- Production execution is `Renderer → Tauri Host → UDS → Agent Daemon`.
  Renderer, Host commands, schedulers, and tenants must not bypass this path.
- Host data and Daemon data have separate SQLite authorities. Renderer never
  opens SQLite directly.
- Host/Daemon wire types have one source in `crates/assistant-protocol`;
  generated frontend bindings under `src/types/generated/` are not hand-edited.
- Workshop iframe and Embed WebView are different trust domains. Never give
  Embed the Workshop Bridge or weaken the iframe sandbox.

## Change discipline

- Use `rtk` for shell commands.
- Reuse existing domain modules, adapters, types, tokens, and test patterns
  before adding abstractions or dependencies.
- Keep user-visible data real, error states explicit, and `src/i18n/zh.ts` /
  `src/i18n/en.ts` synchronized.
- Preserve the five security defenses in `docs/standards/technical/02-security.md`.
- Performance work MUST follow `docs/standards/technical/04-performance.md`
  and include comparable before/after evidence.
- Do not mix unrelated formatting, renames, generated output, or historical
  cleanup into a focused change.

## Verification

Before handoff, run:

```sh
rtk npm run typecheck
rtk npm run lint
rtk npm run test
rtk npm run perf:check
```

Also run the checks for the area touched:

- Rust: `rtk cargo fmt --check` and `rtk cargo test --workspace`.
- Host/Daemon protocol: `rtk npm run protocol:check`.
- Native engine: `rtk npm run verify:native-engine`.
- Extension host: `rtk npm --prefix extension-host run typecheck` and
  `rtk npm --prefix extension-host run test`.
