#!/usr/bin/env bash
# Reproducible Native Engine verification.
#
# The runner writes sanitized, machine-readable evidence. It never accepts
# credentials as CLI arguments and records live-provider absence as not_run,
# not as pass.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

SCRATCH=""
LIVE=0
HEADED_EVIDENCE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --scratch)
      SCRATCH="${2:?--scratch requires a directory}"
      shift 2
      ;;
    --live)
      LIVE=1
      shift
      ;;
    --headed-evidence)
      HEADED_EVIDENCE="${2:?--headed-evidence requires a directory}"
      shift 2
      ;;
    *)
      echo "usage: $0 [--scratch DIR] [--live] [--headed-evidence DIR]" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$SCRATCH" ]]; then
  SCRATCH="${TMPDIR:-/tmp}/natives-native-engine-verification/$(date -u +%Y%m%dT%H%M%SZ)"
fi
mkdir -p "$SCRATCH"
chmod 700 "$SCRATCH" 2>/dev/null || true

sanitize() {
  sed -E \
    -e 's/(sk-[A-Za-z0-9_-]{8,})/[REDACTED_KEY]/g' \
    -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1[REDACTED]/Ig' \
    -e 's/(api[_-]?key|authorization|token)[=:][[:space:]]*[^,[:space:]}]+/\1=[REDACTED]/Ig'
}

write_json() {
  local path="$1"; shift
  printf '%s\n' "$*" > "$SCRATCH/$path"
}

run_step() {
  local name="$1"; shift
  local log="$SCRATCH/$name.log"
  local status
  set +e
  "$@" 2>&1 | sanitize > "$log"
  status=${PIPESTATUS[0]}
  set -e
  printf '{"name":"%s","status":"%s","log":"%s"}\n' \
    "$name" "$([[ "$status" -eq 0 ]] && echo pass || echo fail)" "$name.log" \
    > "$SCRATCH/$name.json"
  tail -n 4 "$log" >&2 || true
  return "$status"
}

overall=0
run_step old-chain bash scripts/daemon/audit-old-symbols.sh || overall=1
run_step protocol-sync npm run protocol:check || overall=1
run_step frontend-tests npm test || overall=1
run_step frontend-typecheck npm run typecheck || overall=1
run_step rust-check-strict env 'RUSTFLAGS=-D warnings' cargo check \
  -p natives -p natives-agent-daemon -p assistant-protocol -p agent-core \
  -p provider-adapters -p capability-gateway || overall=1
run_step protocol-tests cargo test -p assistant-protocol --lib || overall=1
run_step provider-contract cargo test -p provider-adapters --test contract -- --nocapture || overall=1
run_step engine-core-tests cargo test -p agent-core --lib || overall=1
run_step daemon-tests cargo test -p natives-agent-daemon --lib -- --test-threads=1 || overall=1
run_step uds-lifecycle cargo test -p natives-agent-daemon --test uds_run_lifecycle -- --nocapture || overall=1
run_step dual-provider-fixture cargo test -p natives-agent-daemon --test live_engine_e2e dual_provider_engine_fixture_subagent -- --nocapture || overall=1

oauth_status="unsupported"
if sed -n '/^pub const IMPLEMENTED_METHODS/,/^];/p' crates/assistant-protocol/src/v2/methods.rs \
  | rg -q 'mcp\.auth\.oauth(Start|Callback)'; then
  oauth_status="fail"
  overall=1
fi
write_json mcp-oauth.json "{\"status\":\"$oauth_status\",\"oauthStart_advertised\":false,\"oauthCallback_advertised\":false,\"contract\":\"known-but-unsupported\"}"

provider_status() {
  local provider="$1" key_var="$2" base_var="$3"
  local key_present=false base_present=false
  [[ -n "${!key_var:-}" ]] && key_present=true
  [[ -n "${!base_var:-}" ]] && base_present=true
  if [[ "$provider" == "anthropic" ]]; then
    [[ -n "${ANTHROPIC_API_KEY:-}${ANTHROPIC_AUTH_TOKEN:-}" ]] && key_present=true
    [[ -n "${ANTHROPIC_BASE_URL:-}" ]] && base_present=true
  fi
  if [[ "$LIVE" -ne 1 ]]; then
    write_json "provider-$provider-live.json" "{\"provider\":\"$provider\",\"status\":\"not_run\",\"reason\":\"credential_absent_or_live_not_requested\",\"key_present\":$key_present,\"base_present\":$base_present}"
    return 0
  fi
  if [[ "$key_present" != true ]]; then
    write_json "provider-$provider-live.json" "{\"provider\":\"$provider\",\"status\":\"not_run\",\"reason\":\"credential_absent\",\"key_present\":$key_present,\"base_present\":$base_present}"
    overall=1
    return 0
  fi
  local model="${NATIVES_TEST_MODEL:-}"
  [[ "$provider" == "anthropic" ]] && model="${NATIVES_TEST_ANTHROPIC_MODEL:-$model}"
  if [[ -z "$model" ]]; then
    write_json "provider-$provider-live.json" "{\"provider\":\"$provider\",\"status\":\"not_run\",\"reason\":\"model_absent\",\"key_present\":$key_present,\"base_present\":$base_present}"
    overall=1
    return 0
  fi

  local failed=0
  run_live_test() {
    local name="$1" package="$2" test_target="$3" test_name="$4"
    local log="$SCRATCH/provider-$provider-$name.log"
    set +e
    NATIVES_LIVE_E2E=1 NATIVES_LIVE_PROVIDER_ID="$provider" NATIVES_TEST_MODEL="$model" \
      NATIVES_TEST_SCRATCH="$SCRATCH" \
      cargo test -p "$package" --test "$test_target" "$test_name" -- --nocapture --ignored \
      2>&1 | sanitize > "$log"
    local status=${PIPESTATUS[0]}
    set -e
    if [[ "$status" -eq 0 ]]; then
      write_json "provider-$provider-$name.json" "{\"provider\":\"$provider\",\"case\":\"$name\",\"status\":\"pass\",\"model\":\"$model\",\"log\":\"provider-$provider-$name.log\"}"
    else
      write_json "provider-$provider-$name.json" "{\"provider\":\"$provider\",\"case\":\"$name\",\"status\":\"fail\",\"model\":\"$model\",\"log\":\"provider-$provider-$name.log\"}"
      failed=1
    fi
  }

  if [[ "$provider" == "openai_compatible" ]]; then
    run_live_test adapter-text provider-adapters live_openai_compatible_e2e live_text_stream_completes
    run_live_test adapter-tool provider-adapters live_openai_compatible_e2e live_tool_roundtrip_body_and_second_turn
  elif [[ "$provider" == "anthropic" ]]; then
    run_live_test adapter-text provider-adapters live_anthropic_e2e live_text_stream_completes
    run_live_test adapter-tool provider-adapters live_anthropic_e2e live_tool_roundtrip_blocks_and_second_turn
  else
    write_json "provider-$provider-adapter-text.json" "{\"provider\":\"$provider\",\"case\":\"adapter-text\",\"status\":\"not_run\",\"reason\":\"adapter_live_test_not_available\"}"
    write_json "provider-$provider-adapter-tool.json" "{\"provider\":\"$provider\",\"case\":\"adapter-tool\",\"status\":\"not_run\",\"reason\":\"adapter_live_test_not_available\"}"
    failed=1
  fi
  run_live_test engine-text natives-agent-daemon live_engine_e2e live_engine_text_turn
  run_live_test engine-tool natives-agent-daemon live_engine_e2e live_engine_tool_loop
  run_live_test engine-subagent natives-agent-daemon live_engine_e2e live_subagent_task_completes
  run_live_test engine-cancel natives-agent-daemon live_engine_e2e live_engine_cancel_stream
  write_json "provider-$provider-retry-live.json" "{\"provider\":\"$provider\",\"case\":\"engine-retry\",\"status\":\"not_run\",\"reason\":\"requires_controlled_retryable_provider_endpoint\",\"offline_engine_retry_test\":\"engine::tests::retries_retryable_provider_stream_open_errors\"}"
  failed=1

  if [[ "$failed" -eq 0 ]]; then
    write_json "provider-$provider-live.json" "{\"provider\":\"$provider\",\"status\":\"pass\",\"key_present\":$key_present,\"base_present\":$base_present,\"model\":\"$model\"}"
  else
    write_json "provider-$provider-live.json" "{\"provider\":\"$provider\",\"status\":\"fail\",\"key_present\":$key_present,\"base_present\":$base_present,\"model\":\"$model\"}"
    overall=1
  fi
}

provider_status openai_compatible NATIVES_TEST_OPENAI_KEY NATIVES_TEST_OPENAI_BASE
provider_status anthropic NATIVES_TEST_ANTHROPIC_KEY NATIVES_TEST_ANTHROPIC_BASE

if [[ "$LIVE" -eq 1 ]]; then
  openai_key_present=false
  anthropic_key_present=false
  [[ -n "${NATIVES_TEST_OPENAI_KEY:-}" ]] && openai_key_present=true
  [[ -n "${NATIVES_TEST_ANTHROPIC_KEY:-}${ANTHROPIC_API_KEY:-}${ANTHROPIC_AUTH_TOKEN:-}" ]] && anthropic_key_present=true
  if [[ "$openai_key_present" == true && "$anthropic_key_present" == true && -n "${NATIVES_TEST_MODEL:-}" && -n "${NATIVES_TEST_ANTHROPIC_MODEL:-}" ]]; then
    log="$SCRATCH/cross-provider-subagent-live.log"
    set +e
    NATIVES_LIVE_E2E=1 NATIVES_TEST_SCRATCH="$SCRATCH" \
      cargo test -p natives-agent-daemon --test live_engine_e2e \
      live_cross_provider_subagent_openai_parent_anthropic_child -- --nocapture --ignored \
      2>&1 | sanitize > "$log"
    status=${PIPESTATUS[0]}
    set -e
    if [[ "$status" -eq 0 ]]; then
      write_json cross-provider-subagent-live.json "{\"status\":\"pass\",\"parent\":\"openai_compatible\",\"child\":\"anthropic\",\"log\":\"cross-provider-subagent-live.log\"}"
    else
      write_json cross-provider-subagent-live.json "{\"status\":\"fail\",\"parent\":\"openai_compatible\",\"child\":\"anthropic\",\"log\":\"cross-provider-subagent-live.log\"}"
      overall=1
    fi
  else
    write_json cross-provider-subagent-live.json "{\"status\":\"not_run\",\"reason\":\"missing_openai_or_anthropic_credential_or_model\",\"parent\":\"openai_compatible\",\"child\":\"anthropic\",\"openai_key_present\":$openai_key_present,\"anthropic_key_present\":$anthropic_key_present}"
    overall=1
  fi
fi

if [[ -n "$HEADED_EVIDENCE" ]]; then
  log="$SCRATCH/gui-headed-evidence.log"
  set +e
  node scripts/daemon/check-gui-headed-evidence.mjs "$HEADED_EVIDENCE" 2>&1 | sanitize > "$log"
  status=${PIPESTATUS[0]}
  set -e
  if [[ "$status" -eq 0 ]]; then
    write_json gui-headed-evidence.json "{\"status\":\"pass\",\"evidence_dir\":\"$HEADED_EVIDENCE\",\"log\":\"gui-headed-evidence.log\"}"
  else
    write_json gui-headed-evidence.json "{\"status\":\"fail\",\"evidence_dir\":\"$HEADED_EVIDENCE\",\"log\":\"gui-headed-evidence.log\"}"
    overall=1
  fi
else
  write_json gui-headed-evidence.json "{\"status\":\"not_run\",\"reason\":\"headed_evidence_absent\"}"
fi

git_rev="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
write_json manifest.json "{\"git_rev\":\"$git_rev\",\"live_requested\":$LIVE,\"headed_evidence\":\"$HEADED_EVIDENCE\",\"scratch\":\"$SCRATCH\"}"
{
  echo "# Native Engine Verification"
  echo
  echo "- git: $git_rev"
  echo "- scratch: $SCRATCH"
  echo "- live requested: $LIVE"
  echo "- headed evidence: ${HEADED_EVIDENCE:-not_run}"
  echo "- overall: $([[ "$overall" -eq 0 ]] && echo pass || echo fail)"
  echo
  echo "Evidence files are sanitized; provider credentials and OAuth tokens are not recorded."
} > "$SCRATCH/summary.md"

echo "verification scratch: $SCRATCH"
exit "$overall"
