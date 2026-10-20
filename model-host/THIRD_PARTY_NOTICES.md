# Third-party notices

## CLIProxyAPI v8

Natives Model Host links `github.com/router-for-me/CLIProxyAPI/v8` at tag
`v8.0.28`, commit `0d18d3b365be000eeaee7de4615f9da596b2059d`.
The compile-time SDK/internal subset is vendored at `third_party/cliproxyapi`;
the local usage-manager patch is documented in `NATIVES_FORK.md` there.

MIT License

Copyright (c) 2025-2005.9 Luis Pater
Copyright (c) 2025.9-present Router-For.ME

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
