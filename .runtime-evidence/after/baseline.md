# A0 Baseline (pre-remediation)

- commit: b25b3019
- build: cargo test (debug profile)

## B1 pure-text long answer

### 500 chunks
- submit → completed: 761.90 ms
- request → first delta: -1.00 ms
- delta → live publish p50/p95: 0.00/0.00 ms
- terminal tail (last delta → run returned): -1.00 ms
- live events: 0 / durable events: 6
- text_delta count: 0 / message_delta count: 0
- run_event rows / 1000 chunks: 12.0
- run_event payload bytes / 1000 chunks: 22254.0
### 2000 chunks
- submit → completed: 3022.28 ms
- request → first delta: -1.00 ms
- delta → live publish p50/p95: 0.00/0.00 ms
- terminal tail (last delta → run returned): -1.00 ms
- live events: 0 / durable events: 6
- text_delta count: 0 / message_delta count: 0
- run_event rows / 1000 chunks: 3.0
- run_event payload bytes / 1000 chunks: 20313.5

## UDS handshake

- connect+handshake p95: 5.54 ms (per RPC, current behavior)
- handshakes per run (estimate): 8
- note: current UdsAuthority re-connects + handshakes per RPC (authority.rs connect-per-call)
