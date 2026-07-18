#!/usr/bin/env bash
# macOS: start natives-agent-daemon sidecar with secure bootstrap (not logged).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUNTIME_DIR="${NATIVES_RUNTIME_DIR:-$HOME/.natives/runtime}"
mkdir -p "$RUNTIME_DIR"
chmod 700 "$RUNTIME_DIR" 2>/dev/null || true

SOCKET="${NATIVES_DAEMON_SOCKET:-$RUNTIME_DIR/natives-agent.sock}"
# macOS sun_path is short (~104). Auto-fallback to /tmp when path is too long.
if [[ ${#SOCKET} -gt 100 ]]; then
  SOCKET="/tmp/nuds-$(printf '%s' "$RUNTIME_DIR" | shasum -a 256 2>/dev/null | cut -c1-12 || echo $$).sock"
  echo "NOTE: socket path shortened to $SOCKET (UDS sun_path limit)"
fi
PID_FILE="${NATIVES_DAEMON_PID:-$RUNTIME_DIR/natives-agent.pid}"
BOOTSTRAP_FILE="${NATIVES_DAEMON_BOOTSTRAP_FILE:-$RUNTIME_DIR/bootstrap.token}"
DB_PATH="${NATIVES_DB_PATH:-$HOME/.natives/natives.db}"
BIN="${NATIVES_DAEMON_BIN:-}"

if [[ -z "$BIN" ]]; then
  if [[ -x "$ROOT/target/release/natives-agent-daemon" ]]; then
    BIN="$ROOT/target/release/natives-agent-daemon"
  elif [[ -x "$ROOT/target/debug/natives-agent-daemon" ]]; then
    BIN="$ROOT/target/debug/natives-agent-daemon"
  else
    BIN="natives-agent-daemon"
  fi
fi

# Clean stale socket if pid is dead
if [[ -S "$SOCKET" ]]; then
  if [[ -f "$PID_FILE" ]]; then
    old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [[ -n "${old_pid:-}" ]] && kill -0 "$old_pid" 2>/dev/null; then
      echo "daemon already running pid=$old_pid socket=$SOCKET"
      exit 0
    fi
  fi
  rm -f "$SOCKET"
fi

# Bootstrap token via secure pipe file (0600) — never echo to logs
if [[ -z "${NATIVES_DAEMON_BOOTSTRAP:-}" ]]; then
  BOOTSTRAP="$(openssl rand -hex 32 2>/dev/null || python3 -c 'import secrets;print(secrets.token_hex(32))')"
  umask 077
  printf '%s' "$BOOTSTRAP" >"$BOOTSTRAP_FILE"
  chmod 600 "$BOOTSTRAP_FILE"
  export NATIVES_DAEMON_BOOTSTRAP="$BOOTSTRAP"
else
  umask 077
  printf '%s' "$NATIVES_DAEMON_BOOTSTRAP" >"$BOOTSTRAP_FILE"
  chmod 600 "$BOOTSTRAP_FILE"
fi

export NATIVES_DAEMON_MODE="${NATIVES_DAEMON_MODE:-uds}"
export NATIVES_DAEMON_SOCKET="$SOCKET"
export NATIVES_RUNTIME_DIR="$RUNTIME_DIR"
export NATIVES_DB_PATH="$DB_PATH"
export NATIVES_REQUIRE_UDS="${NATIVES_REQUIRE_UDS:-1}"

# Daemon reads NATIVES_DAEMON_SOCKET / NATIVES_DAEMON_BOOTSTRAP from env (no CLI args).
nohup env \
  NATIVES_DAEMON_MODE="$NATIVES_DAEMON_MODE" \
  NATIVES_DAEMON_SOCKET="$SOCKET" \
  NATIVES_DAEMON_BOOTSTRAP="$NATIVES_DAEMON_BOOTSTRAP" \
  NATIVES_RUNTIME_DIR="$RUNTIME_DIR" \
  NATIVES_DB_PATH="$DB_PATH" \
  NATIVES_REQUIRE_UDS="$NATIVES_REQUIRE_UDS" \
  "$BIN" \
  >"$RUNTIME_DIR/daemon.stdout.log" 2>"$RUNTIME_DIR/daemon.stderr.log" &
echo $! >"$PID_FILE"
chmod 600 "$PID_FILE"

# Readiness: socket appears
for _ in $(seq 1 50); do
  if [[ -S "$SOCKET" ]]; then
    echo "daemon ready socket=$SOCKET pid=$(cat "$PID_FILE")"
    echo "bootstrap_file=$BOOTSTRAP_FILE (not printed)"
    exit 0
  fi
  sleep 0.1
done

echo "daemon failed to create socket within 5s" >&2
exit 1
