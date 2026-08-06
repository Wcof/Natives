#!/usr/bin/env bash
# T11 — Daemon idle CPU/RSS evidence (matrix item 4).
#
# Spawns the daemon binary on a throwaway runtime dir, connects once to
# exercise startup, then samples `ps` RSS / %CPU for 60 s of idle. Writes a
# machine-readable JSON report + summary to NATIVES_PERF_SCRATCH (or a temp dir).
#
# The daemon never touches ~/.natives: every NATIVES_* env var points at the
# scratch runtime dir. This script must be run from the repository root.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

# Resolve the cargo target dir (the worktree shares one via .cargo/config.toml).
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null \
  || echo "$ROOT/target")"
BIN="${NATIVES_DAEMON_BIN:-$TARGET_DIR/debug/natives-agent-daemon}"
if [[ ! -x "$BIN" ]]; then
  echo "daemon binary missing: $BIN (build with: cargo build -p natives-agent-daemon)" >&2
  exit 2
fi

SCRATCH="${NATIVES_PERF_SCRATCH:-$ROOT/target/perf-evidence/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$SCRATCH"
RUN="$SCRATCH/run"
mkdir -p "$RUN"

SOCK="/tmp/nidle-$(uuidgen | cut -c1-8).sock"
BOOTSTRAP="boot-$(uuidgen | cut -c1-8)"
export NATIVES_DAEMON_SOCKET="$SOCK"
export NATIVES_DAEMON_BOOTSTRAP="$BOOTSTRAP"
export NATIVES_DB_PATH="$RUN/natives.db"
export NATIVES_ASSISTANT_DB_PATH="$RUN/assistant.db"
export NATIVES_RUNTIME_DIR="$RUN"
export NATIVES_DAEMON_FIXTURE="1"

SAMPLE_SECS="${NATIVES_IDLE_SECONDS:-60}"

"$BIN" >"$RUN/daemon.stdout.log" 2>"$RUN/daemon.stderr.log" &
DPID=$!
trap 'kill $DPID 2>/dev/null || true; rm -f "$SOCK"' EXIT

# Wait for the socket.
for _ in $(seq 1 100); do
  [[ -S "$SOCK" ]] && break
  kill -0 $DPID 2>/dev/null || { echo "daemon exited early" >&2; cat "$RUN/daemon.stderr.log" >&2; exit 1; }
  sleep 0.1
done
[[ -S "$SOCK" ]] || { echo "socket never appeared" >&2; exit 1; }

# One handshake to settle startup.
"$ROOT/scripts/perf/ping-daemon.mjs" "$SOCK" "$BOOTSTRAP" >/dev/null 2>&1 || true

SAMPLES=""
for _ in $(seq 1 $((SAMPLE_SECS / 5))); do
  sleep 5
  read -r RSS CPU <<< "$(ps -o rss=,pcpu= -p $DPID 2>/dev/null | awk '{print $1, $2}')"
  [[ -n "$RSS" ]] || RSS=0
  [[ -n "$CPU" ]] || CPU=0
  SAMPLES="$SAMPLES$RSS $CPU\n"
done

printf '%b' "$SAMPLES" > "$SCRATCH/idle-samples.txt"

python3 - "$SCRATCH" "$DPID" "$SAMPLE_SECS" <<'PY'
import json, statistics, sys, time
scratch, pid, secs = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
rows = []
for line in open(f"{scratch}/idle-samples.txt"):
    line = line.strip()
    if not line:
        continue
    rss_kb, cpu = line.split()
    rows.append({"rss_kb": int(rss_kb), "cpu_pct": float(cpu)})
rss = [r["rss_kb"] for r in rows]
cpu = [r["cpu_pct"] for r in rows]
def pct(a, p):
    s = sorted(a)
    return s[max(0, min(len(s)-1, int(len(s)*p)))]
report = {
    "metric": "daemon_idle_60s",
    "build": "debug",
    "process": "natives-agent-daemon",
    "samples": len(rows),
    "rss_kb": {
        "median": statistics.median(rss) if rss else 0,
        "p75": pct(rss, 0.75) if rss else 0,
        "max": max(rss) if rss else 0,
    },
    "cpu_pct": {
        "median": statistics.median(cpu) if cpu else 0,
        "p95": pct(cpu, 0.95) if cpu else 0,
        "max": max(cpu) if cpu else 0,
    },
    "samples_raw": rows,
}
json.dump(report, open(f"{scratch}/idle-evidence.json", "w"), indent=1)
print(f"daemon idle {len(rows)} samples over {secs}s:")
print(f"  RSS median {report['rss_kb']['median']} KB, max {report['rss_kb']['max']} KB")
print(f"  CPU median {report['cpu_pct']['median']}%, p95 {report['cpu_pct']['p95']}%, max {report['cpu_pct']['max']}%")
PY

echo "idle evidence: $SCRATCH/idle-evidence.json"
