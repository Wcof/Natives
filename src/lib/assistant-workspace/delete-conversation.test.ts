/**
 * Conversation delete UX contracts.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const workbench = readFileSync(
  resolve(process.cwd(), 'src/components/assistant/AssistantWorkbench.tsx'),
  'utf8',
);
const context = readFileSync(
  resolve(process.cwd(), 'src/components/assistant/AssistantWorkspaceContext.tsx'),
  'utf8',
);
const host = readFileSync(
  resolve(process.cwd(), 'src-tauri/src/assistant_service.rs'),
  'utf8',
);
// conversation.* is Daemon authority: the host router only forwards, the delete
// itself (and the usage_stats fold) lives in the agent daemon.
const daemonStore = readFileSync(
  resolve(process.cwd(), 'src-agent-daemon/src/conversation_store.rs'),
  'utf8',
);
const daemonSchema = readFileSync(
  resolve(process.cwd(), 'src-agent-daemon/src/storage/migrations.rs'),
  'utf8',
);

test('workbench delete optimistically drops from store and navigation groups', () => {
  assert.match(workbench, /deleteConversation:\s*async \(id\)/);
  assert.match(workbench, /conversations\/remove/);
  assert.match(workbench, /conversations:\s*g\.conversations\.filter/);
  // Optimistic path runs before awaiting host RPC.
  const deleteFn = workbench.slice(workbench.indexOf('deleteConversation: async (id)'));
  const dropIdx = deleteFn.indexOf('dropFromUi');
  const requestIdx = deleteFn.indexOf("gateway.request('conversation.delete'");
  assert.ok(dropIdx >= 0 && requestIdx > dropIdx, 'UI drop must precede host request');
});

test('shell fallback delete also optimistically updates navigation', () => {
  assert.match(context, /deleteConversation:\s*async \(id\)/);
  assert.match(context, /dropFromNav/);
  assert.match(context, /conversations:\s*g\.conversations\.filter/);
});

test('host forwards conversation.delete to daemon authority', () => {
  const hostOwned = host.slice(host.indexOf('pub(crate) fn is_host_owned_method'));
  assert.equal(/"conversation\./.test(hostOwned.slice(0, hostOwned.indexOf('}'))), false);
  assert.match(host, /method\.starts_with\("conversation\."\)/);
  assert.match(host, /daemon_authority::request\(method, params\.clone\(\)\)/);
});

test('daemon delete always hard-deletes even when cleanup is pending', () => {
  const fn = daemonStore.slice(daemonStore.indexOf('async fn delete(params: Value)'));
  const hardDeleteIdx = fn.indexOf('DELETE FROM conversation WHERE id = ?1');
  const earlyCleanup = fn.indexOf('if cleanup_pending');
  assert.ok(hardDeleteIdx > 0);
  // Either no early return, or hard delete appears before any such gate.
  if (earlyCleanup > 0) {
    assert.ok(hardDeleteIdx < earlyCleanup, 'hard delete must not be skipped by cleanup_pending');
  }
  // Hard delete must preserve billing aggregates, and the fold has to happen
  // before CASCADE removes the run/message rows it reads.
  const foldIdx = fn.indexOf('fold_conversation_tokens_into_usage_stats(&conn');
  assert.ok(foldIdx > 0 && foldIdx < hardDeleteIdx, 'tokens must be folded before delete');
  assert.match(fn, /usage_stats_preserved/);
  assert.match(fn, /hard_deleted/);
});

test('daemon delete also clears conversation-scoped tool_calls and artifacts', () => {
  // No explicit DELETE needed: both hang off the conversation via FK CASCADE
  // (artifact directly, tool_call through run).
  assert.match(
    daemonSchema,
    /CREATE TABLE IF NOT EXISTS artifact \([^;]*conversation_id TEXT NOT NULL REFERENCES conversation\(id\) ON DELETE CASCADE/,
  );
  assert.match(
    daemonSchema,
    /CREATE TABLE IF NOT EXISTS tool_call \([^;]*run_id TEXT NOT NULL REFERENCES run\(id\) ON DELETE CASCADE/,
  );
  assert.match(
    daemonSchema,
    /CREATE TABLE IF NOT EXISTS run \([^;]*conversation_id TEXT NOT NULL REFERENCES conversation\(id\) ON DELETE CASCADE/,
  );
});
