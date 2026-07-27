---
paths:
  - "src/**/*.{ts,tsx,css}"
  - "src-tauri/**/*.rs"
  - "src-agent-daemon/**/*.rs"
  - "crates/**/*.rs"
  - "extension-host/src/**/*.ts"
  - "scripts/perf/**"
  - "package.json"
  - "next.config.ts"
---

The normative performance rules live in `docs/standards/technical/04-performance.md`.
Read that file before changing any matched path and run `npm run perf:check` before handoff.
