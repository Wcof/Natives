#!/usr/bin/env bash
# T11 — Reproducible daemon-side performance & observability evidence.
#
# Runs (1) the standard-dataset RPC/primitive matrix and (2) the 60 s idle
# CPU/RSS sample, writing machine-readable evidence + a summary to
# NATIVES_PERF_SCRATCH (default: <repo>/target/perf-evidence/<timestamp>).
#
# usage:
#   bash scripts/perf/daemon-evidence.sh
#
# The matrix test is `#[ignore]` by design (a benchmark harness, not a unit
# gate). It builds the standard dataset (500 conv x 2000 msgs, 20k events) on
# a throwaway DB and measures the real UDS RPC paths.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

SCRATCH="${NATIVES_PERF_SCRATCH:-$ROOT/target/perf-evidence/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$SCRATCH"
export NATIVES_PERF_SCRATCH="$SCRATCH"

echo "== daemon perf evidence =="
echo "scratch: $SCRATCH"

echo "== matrix (dataset RPC + bounded primitives) =="
cargo test -p natives-agent-daemon --test perf_evidence -- --ignored --nocapture 2>&1 \
  | tail -n 12 || true

echo "== idle 60s CPU/RSS =="
bash scripts/perf/daemon-idle-evidence.sh 2>&1 | tail -n 6 || true

echo
echo "evidence written to: $SCRATCH"
ls -la "$SCRATCH"/*.json "$SCRATCH"/*.md 2>/dev/null || true
echo
echo "summary: $SCRATCH/summary.md"
