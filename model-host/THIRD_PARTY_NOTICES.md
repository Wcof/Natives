# Third-party notices

The Model Host no longer links the CLIProxyAPI SDK, internal subset, or Natives
patches; that vendored tree was removed under ADR-0044. Historical usage import
formats and price-source URLs that mention EasyCLIProxyAPI/CLIProxyAPI are
compatibility references only, not runtime dependencies.

## PanWatch (source migration donor)

The Fund module's auto-information capability is a Rust/JavaScript source
migration of logic from the PanWatch project
(`https://github.com/TNT-Likely/PanWatch`, local clone commit
`00b345b05145705afea9ea9d5cecc090c009d83c`), files: marketdata vendors
(news/events), events_collector, market news API/ranker, research
signal_pack/context_builder/context_store/insights/structured_output.
Migrated behavior was rewritten to Natives constraints (TLS verification
kept, no `verify=False`, no environment proxy, explicit failure states
instead of empty-array success, UTC+8 source-time parsing without fake
"now" timestamps, no sentiment-to-trade inference). Only the logic listed
in `plan/00-baseline-and-migration-map.md` §3 was migrated; PanWatch's
Python/FastAPI/React/SQLAlchemy stack does not ship with Natives.

MIT License

Copyright (c) 2026 sunxiao0721

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
