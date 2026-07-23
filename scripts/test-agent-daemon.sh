#!/usr/bin/env bash
# Daemon lib tests. Use --serial or NATIVES_TEST_SERIAL=1 for env-race debugging.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
THREADS="${NATIVES_TEST_THREADS:-0}"
EXTRA=()
if [[ "${1:-}" == "--serial" ]] || [[ "${NATIVES_TEST_SERIAL:-}" == "1" ]]; then
  THREADS=1
  if [[ "${1:-}" == "--serial" ]]; then shift; fi
fi
if [[ "$THREADS" != "0" ]]; then EXTRA=(-- --test-threads="$THREADS"); fi
echo "[test-agent-daemon] cargo test -p natives-agent-daemon --lib threads=${THREADS:-default} $*"
exec cargo test -p natives-agent-daemon --lib "$@" "${EXTRA[@]}"
