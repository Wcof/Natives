---
paths:
  - "src/**/*.{ts,tsx,css}"
  - "src-tauri/**/*.rs"
  - "src-agent-daemon/**/*.rs"
  - "scripts/perf/**"
  - "package.json"
---

The normative performance rules live in `docs/standards/technical/04-performance.md`.
Read that file before changing any matched path and run `npm run perf:check` before handoff.
