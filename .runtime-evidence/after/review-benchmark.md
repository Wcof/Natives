# Integration Review Benchmark (2026-08-08, HEAD 662f1cca + local A8/memory_only changes)

| Metric | Before (A0) | After (aba50b06) | Review re-run | Change vs before |
|---|---:|---:|---:|---:|
| submit→completed 500 chunks | 1612 ms | 707 ms | 760 ms | −53% |
| submit→completed 2000 chunks | 7852 ms | 2855 ms | 3008 ms | −62% |
| durable run_event rows | 1006/4006 | 6 | 6 | live delta SQLite writes = 0 |
| new message_delta emits | 500 | 0 | 0 | 0 |

B2 (readonly loop: no ledger/checkpoint) / B3 (settle atomicity) / B5 (stream_watch + handshake reuse): all pass.
