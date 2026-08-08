# A0 Baseline (pre-remediation)

- commit: 4b8193cd
- build: cargo test (debug profile)

## B1 pure-text long answer

### 500 chunks
- submit → completed: 1612.10 ms
- request → first delta: 0.48 ms
- delta → live publish p50/p95: 1.54/3.42 ms
- terminal tail (last delta → run returned): 5.77 ms
- live events: 500 / durable events: 506
- text_delta count: 500 / message_delta count: 500
- run_event rows / 1000 chunks: 2012.0
- run_event payload bytes / 1000 chunks: 5210168.0
### 2000 chunks
- submit → completed: 7851.53 ms
- request → first delta: 0.33 ms
- delta → live publish p50/p95: 2.49/4.51 ms
- terminal tail (last delta → run returned): 4.71 ms
- live events: 2000 / durable events: 2006
- text_delta count: 2000 / message_delta count: 2000
- run_event rows / 1000 chunks: 2003.0
- run_event payload bytes / 1000 chunks: 19704911.0

## UDS handshake

- connect+handshake p95: 0.89 ms (per RPC, current behavior)
- handshakes per run (estimate): 8
- note: current UdsAuthority re-connects + handshakes per RPC (authority.rs connect-per-call)
