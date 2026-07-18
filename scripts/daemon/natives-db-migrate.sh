#!/usr/bin/env bash
# NATIVES_DB_PATH check / backup helper (Phase 2 ops).
set -euo pipefail

DB="${NATIVES_DB_PATH:-$HOME/.natives/natives.db}"
CMD="${1:-check}"

case "$CMD" in
  check)
    if [[ ! -f "$DB" ]]; then
      echo "MISSING: $DB" >&2
      exit 1
    fi
    if [[ ! -r "$DB" ]]; then
      echo "UNREADABLE: $DB" >&2
      exit 1
    fi
    # Absolute path recommendation
    if [[ "$DB" != /* && "$DB" != [A-Za-z]:* ]]; then
      echo "WARN: prefer absolute NATIVES_DB_PATH (got $DB)" >&2
    fi
    if command -v sqlite3 >/dev/null 2>&1; then
      mode=$(sqlite3 "$DB" "PRAGMA journal_mode;" 2>/dev/null || echo "?")
      echo "OK path=$DB journal_mode=$mode"
      sqlite3 "$DB" "PRAGMA schema_version;" 2>/dev/null | awk '{print "schema_version="$1}'
    else
      echo "OK path=$DB (sqlite3 not installed; skipped schema check)"
    fi
    ;;
  backup)
    ts=$(date +%Y%m%d%H%M%S)
    dest="${2:-$DB.backup.$ts}"
    cp -p "$DB" "$dest"
    # Also copy WAL/SHM if present
    [[ -f "$DB-wal" ]] && cp -p "$DB-wal" "$dest-wal" || true
    [[ -f "$DB-shm" ]] && cp -p "$DB-shm" "$dest-shm" || true
    echo "BACKUP $DB -> $dest"
    ;;
  *)
    echo "usage: $0 check|backup [dest]" >&2
    exit 2
    ;;
esac
