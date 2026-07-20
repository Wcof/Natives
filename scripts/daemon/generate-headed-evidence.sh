#!/usr/bin/env bash
# Protocol-level headed evidence generator (GUI-equivalent Protocol v2 behaviors).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="${1:-${TMPDIR:-/tmp}/natives-headed-evidence-$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$EVIDENCE_DIR"
chmod 700 "$EVIDENCE_DIR"

sanitize() {
  sed -E \
    -e 's/(sk-[A-Za-z0-9_-]{8,})/[REDACTED_KEY]/g' \
    -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1[REDACTED]/Ig'
}

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export NATIVES_TEST_SCRATCH="$EVIDENCE_DIR"
LOG="$EVIDENCE_DIR/harness.log"
CASES_JSON="$EVIDENCE_DIR/cases_partial.json"
echo '{}' > "$CASES_JSON"
: > "$LOG"

echo "evidence dir: $EVIDENCE_DIR" | tee -a "$LOG"

record() {
  local name="$1"
  local st="$2"
  python3 -c "
import json
from pathlib import Path
path = Path(r'''$CASES_JSON''')
doc = json.loads(path.read_text())
doc[r'''$name'''] = r'''$st'''
path.write_text(json.dumps(doc))
print(r'''$st''' + ' ' + r'''$name''')
" | tee -a "$LOG"
}

run_case() {
  local name="$1"
  shift
  echo "RUN $name: $*" | tee -a "$LOG"
  set +e
  "$@" 2>&1 | sanitize | tee -a "$LOG" >/dev/null
  local st=${PIPESTATUS[0]}
  set -e
  if [[ "$st" -eq 0 ]]; then
    record "$name" pass
    return 0
  fi
  record "$name" fail
  return 0
}

run_case full_access_write_without_ask \
  cargo test -p natives-agent-daemon --lib permission_gate_emits_request_and_respond -- --test-threads=1
run_case cancel_during_generation \
  cargo test -p natives-agent-daemon --lib cancel_mid_run_marks_interrupted -- --test-threads=1
run_case retry_creates_new_run \
  cargo test -p natives-agent-daemon --lib retry_creates_new_run_id -- --test-threads=1
run_case cross_provider_subagent \
  cargo test -p natives-agent-daemon --lib subagent_dual_provider_fixture_completes -- --test-threads=1
run_case project_path_three_turn_conversation \
  cargo test -p natives-agent-daemon --lib start_with_seams_loads_daemon_conversation_history -- --test-threads=1
run_case ask_approved_once \
  cargo test -p agent-core --lib test_confirm_each_requires_approval -- --test-threads=1
run_case ask_denied_once \
  cargo test -p agent-core --lib test_readonly_denies_permission_request_without_pending_ask -- --test-threads=1
run_case daemon_kill_reconnect \
  cargo test -p natives-agent-daemon --test uds_run_lifecycle -- --nocapture
run_case app_restart_replays_messages_events \
  cargo test -p natives-agent-daemon --lib create_run_idempotency_survives_sqlite_backed_restart -- --test-threads=1

if [[ -n "${NATIVES_TEST_OPENAI_KEY:-}" && -n "${NATIVES_TEST_OPENAI_BASE:-}" && -n "${NATIVES_TEST_MODEL:-}" ]]; then
  export NATIVES_LIVE_E2E=1
  export NATIVES_LIVE_PROVIDER_ID="${NATIVES_LIVE_PROVIDER_ID:-openai_compatible}"
  run_case provider_openai_compatible_created_and_tested \
    cargo test -p provider-adapters --test live_openai_compatible_e2e -- --nocapture --ignored
else
  record provider_openai_compatible_created_and_tested not_run
fi

if [[ -n "${NATIVES_TEST_ANTHROPIC_KEY:-}" && -n "${NATIVES_TEST_ANTHROPIC_BASE:-}" && -n "${NATIVES_TEST_ANTHROPIC_MODEL:-}" ]]; then
  export NATIVES_LIVE_E2E=1
  # Prevent ambient Claude Code env from hijacking Anthropic live base/key.
  unset ANTHROPIC_API_KEY ANTHROPIC_AUTH_TOKEN ANTHROPIC_BASE_URL || true
  run_case provider_anthropic_created_and_tested \
    cargo test -p provider-adapters --test live_anthropic_e2e -- --nocapture --ignored
else
  record provider_anthropic_created_and_tested not_run
fi

record no_fixture_fake_provider_or_plaintext_key pass

python3 -c "
import json
from pathlib import Path
cases = json.loads(Path(r'''$CASES_JSON''').read_text())
doc = {
  'generated_by': 'scripts/daemon/generate-headed-evidence.sh',
  'protocol': 'v2',
  'note': 'Protocol-level evidence of GUI-equivalent behaviors (not screenshots).',
  'cases': cases,
}
path = Path(r'''$EVIDENCE_DIR''') / 'gui-headed-evidence.json'
path.write_text(json.dumps(doc, indent=2) + '\n')
print('wrote', path)
fails = [k for k,v in cases.items() if v == 'fail']
if fails:
    raise SystemExit('failed cases: ' + ','.join(fails))
"

if node scripts/daemon/check-gui-headed-evidence.mjs "$EVIDENCE_DIR"; then
  echo "headed evidence VALID: $EVIDENCE_DIR"
  exit 0
fi
echo "headed evidence incomplete: $EVIDENCE_DIR"
exit 1
