# Performance delta (A0 baseline → integration) — re-run 2026-08-08

| Metric | Before | After | Change |
|---|---:|---:|---|
| submit→completed (chunks_500) | 1612 ms | 736 ms | -54% |
| run_event rows/1000 chunks (chunks_500) | 2012 | 12.0 | −99% |
| payload bytes/1000 chunks (chunks_500) | 5.21 MB | 22.3 KB | −100% |
| submit→completed (chunks_2000) | 7852 ms | 2854 ms | -64% |
| run_event rows/1000 chunks (chunks_2000) | 2003 | 3.0 | −100% |
| payload bytes/1000 chunks (chunks_2000) | 19.70 MB | 20.3 KB | −100% |
| new MessageDelta emits (500) | 500 | 0 | **0** |
| durable run_event rows (500) | 1006 | 6 | **live delta SQLite writes = 0** |
