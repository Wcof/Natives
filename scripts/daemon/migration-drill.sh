#!/usr/bin/env bash
# Split-DB migration drill: backup → migrate → verify → idempotent re-run.
# Never copies provider keys. Uses a temp workspace only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

SCRATCH="${1:-${TMPDIR:-/tmp}/natives-migration-drill-$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$SCRATCH"
chmod 700 "$SCRATCH"

sanitize() {
  sed -E \
    -e 's/(sk-[A-Za-z0-9_-]{8,})/[REDACTED_KEY]/g' \
    -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1[REDACTED]/Ig'
}

echo "migration drill scratch: $SCRATCH"

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
set +e
cargo test -p natives-agent-daemon --lib storage:: -- --test-threads=1 --nocapture \
  2>&1 | sanitize | tee "$SCRATCH/cargo-migration-tests.log"
status=${PIPESTATUS[0]}
set -e

if [[ "$status" -ne 0 ]]; then
  echo '{"status":"fail","phase":"unit_tests"}' > "$SCRATCH/result.json"
  exit 1
fi

# Ops backup helper smoke (missing default db is ok — just logs)
set +e
bash scripts/daemon/natives-db-migrate.sh check 2>&1 | sanitize | tee "$SCRATCH/natives-db-check.log" || true
set -e

python3 - <<PY
import json
from pathlib import Path
scratch = Path("$SCRATCH")
log = scratch / "cargo-migration-tests.log"
text = log.read_text() if log.exists() else ""
passed = "0 failed" in text and "test result: ok" in text
split_ok = "split_db_migration" in text
legacy_ok = "legacy_migration" in text
(scratch / "result.json").write_text(json.dumps({
  "status": "pass" if passed else "fail",
  "phases": {
    "split_db_migration_unit": "pass" if split_ok else "unknown",
    "legacy_migration_unit": "pass" if legacy_ok else "unknown",
    "idempotent_rerun": "covered_by_unit",
    "keys_never_copied": "asserted_in_split_db_migration::never_requires_key_tables",
    "backup_helper": "smoke_logged",
  },
  "scratch": str(scratch),
}, indent=2) + "\n")
print("result:", "pass" if passed else "fail")
raise SystemExit(0 if passed else 1)
PY
