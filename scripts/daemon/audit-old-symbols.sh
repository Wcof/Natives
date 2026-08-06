#!/usr/bin/env bash
# Native Engine cutover audit.
#
# This is intentionally a hard gate: a retired execution file or a residual
# production reference is a failure, even when the old implementation is a
# fail-closed stub.  Tests and historical documents are outside this gate.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

fail=0
report() { echo "AUDIT: $*"; }
assert_absent() {
  local path="$1"
  if [[ -e "$path" ]]; then
    report "FAIL: retired file still exists: $path"
    fail=1
  else
    report "OK: retired file absent: $path"
  fi
}
assert_no_production_match() {
  local label="$1"
  local pattern="$2"
  shift 2
  if rg -n --glob '!**/*.test.*' --glob '!**/tests/**' "$pattern" "$@" 2>/dev/null; then
    report "FAIL: residual production reference ($label)"
    fail=1
  else
    report "OK: no production reference ($label)"
  fi
}

# Physical deletion is the preferred retirement policy for the old Native
# execution graph.  Capability data used by settings is not an execution
# authority and is audited separately by the call-site checks below.
for retired in \
  src-tauri/src/assistant_stream_proxy.rs \
  src-tauri/src/assistant_executor.rs \
  src-tauri/src/agent_engine_bridge.rs \
  src-tauri/src/runtime/native_runtime.rs \
  src-tauri/src/runtime/native/agent_loop.rs \
  src-tauri/src/runtime/native/stream_provider.rs \
  src-tauri/src/runtime/native/context_assembler.rs; do
  assert_absent "$retired"
done

# No old execution authority may be imported, registered, or exposed as IPC.
assert_no_production_match "retired Rust modules" \
  'assistant_stream_proxy|agent_engine_bridge|NativeRuntime|runtime_list_catalog|cancel_stream' \
  src-tauri/src
assert_no_production_match "retired frontend IPC" \
  'cancel_stream|runtime_list_catalog' \
  src/lib/tauri-adapter.ts

# 1) Frontend must not call streamChat as execution entry.
if rg -n --glob '!**/node_modules/**' --glob '!**/*.test.ts' --glob '!**/*.test.tsx' \
  '\.streamChat\s*\(|streamChat\s*\(' src/components src/lib/assistant-workspace 2>/dev/null | \
  rg -v 'no streamChat|never call|not call|streamChat is retired|false' ; then
  report "FAIL: UI still invokes streamChat"
  fail=1
else
  report "OK: UI components do not invoke streamChat"
fi

# 1b) tauri-adapter must not contain the retired stream API.
if rg -n "cmd\(\s*['\"]stream_chat['\"]" src/lib/tauri-adapter.ts 2>/dev/null; then
  report "FAIL: tauri-adapter still invokes stream_chat IPC"
  fail=1
else
  report "OK: tauri-adapter does not IPC stream_chat"
fi

# 1c) stream_chat must not be registered in Tauri generate_handler.
if rg -n 'assistant_stream_proxy::stream_chat' src-tauri/src/lib.rs 2>/dev/null; then
  report "FAIL: stream_chat still registered in Tauri invoke_handler"
  fail=1
else
  report "OK: stream_chat not in Tauri invoke_handler"
fi

# 2) No old AgentLoop / bridge execution call sites remain anywhere in Rust.
if rg -n --glob '!**/*.test.*' --glob '!**/tests/**' \
  'AgentLoop::new|AgentLoop::run|loop_runner\.run\(|spawn_run\(|request_cancel\(' src-tauri/src 2>/dev/null; then
  report "FAIL: residual old engine execution call site"
  fail=1
else
  report "OK: no old engine execution call sites found"
fi

# 3) Old executor definitions may remain only as capability implementation
# shims during migration; they must not be wired as a run authority or settings
# catalog source.
if rg -n --glob '!**/*.test.*' --glob '!**/tests/**' \
  'assistant_executor::(execute_tool|run_agentic_loop)|crate::assistant_executor::(execute_tool|run_agentic_loop)' \
  src-tauri/src 2>/dev/null; then
  report "FAIL: assistant_executor still owns production execution"
  fail=1
else
  report "OK: assistant_executor has no production run call site"
fi

if rg -n --glob '!**/*.test.*' --glob '!**/tests/**' \
  'assistant_executor::default_enabled_tools|crate::assistant_executor::default_enabled_tools' \
  src-tauri/src 2>/dev/null; then
  report "FAIL: settings/catalog still sourced from assistant_executor"
  fail=1
else
  report "OK: settings/catalog do not depend on assistant_executor"
fi

# 4) Capability flags honesty — a flag must not be `false` while the matching
#    methods are actually implemented (R-T5: advertisement ⊆ implementation).
#    The daemon honestly does not implement scheduler/extensions, so `false`
#    there is correct as long as those methods are absent from IMPLEMENTED_METHODS.
cap_file="crates/assistant-protocol/src/v2/capabilities.rs"
impl_methods="$(sed -n '/^pub const IMPLEMENTED_METHODS/,/^];/p' crates/assistant-protocol/src/v2/methods.rs)"
honest=1
if rg -n 'mcp: false' "$cap_file" >/dev/null && echo "$impl_methods" | rg -q '"(mcp)\.'; then
  report "FAIL: mcp flag false while mcp methods are implemented"; fail=1; honest=0
fi
if rg -n 'extensions: false' "$cap_file" >/dev/null && echo "$impl_methods" | rg -q '"(extensions|extension)\.[a-z]'; then
  report "FAIL: extensions flag false while extension methods are implemented"; fail=1; honest=0
fi
if rg -n 'scheduler: false' "$cap_file" >/dev/null && echo "$impl_methods" | rg -q '"scheduler\.'; then
  report "FAIL: scheduler flag false while scheduler methods are implemented"; fail=1; honest=0
fi
if [ "$honest" -eq 1 ]; then
  report "OK: capability flags honest vs IMPLEMENTED_METHODS (unimplemented scheduler/extensions correctly false)"
fi

# 5) Production default mode is uds
if rg -n '"uds"\.into\(\)|NATIVES_DAEMON_MODE.*uds' src-agent-daemon/src/client.rs src-tauri/src/lib.rs 2>/dev/null | head -3; then
  report "OK: production default uds present"
else
  report "FAIL: production default uds not found"
  fail=1
fi

# 6) UDS ordinary RPCs must not share one cached client; subscribe/slow RPC must not block cancel.
if rg -n 'Mutex<.*DaemonClient|Option<DaemonClient>' src-agent-daemon/src/authority.rs 2>/dev/null; then
  report "FAIL: UDS authority caches a DaemonClient"
  fail=1
else
  report "OK: UDS authority uses per-RPC authenticated clients"
fi

# 7) Provider protocol must be explicit; never infer from provider type / URL.
if rg -n 'api_protocol\.as_deref\(\)\.unwrap_or\(|normalize_api_protocol\(input\.provider_type|normalize_api_protocol\(&input\.provider_type' src-tauri/src/commands/provider.rs 2>/dev/null; then
  report "FAIL: provider commands infer missing api_protocol"
  fail=1
else
  report "OK: provider commands require explicit api_protocol"
fi

echo "----"
if [[ "$fail" -ne 0 ]]; then
  report "RESULT=FAIL"
  exit 1
fi
report "RESULT=PASS"
exit 0
