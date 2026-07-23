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

test('host delete always hard-deletes even when daemon cleanup is pending', () => {
  assert.match(host, /DELETE FROM assistant_conversations WHERE id = \?1/);
  // Must not early-return on cleanup_pending before hard delete.
  const fn = host.slice(host.indexOf('async fn handle_conversation_delete'));
  const hardDeleteIdx = fn.indexOf('DELETE FROM assistant_conversations');
  const earlyCleanup = fn.indexOf('if cleanup_pending');
  // Either no early return, or hard delete appears before any such gate.
  assert.ok(hardDeleteIdx > 0);
  if (earlyCleanup > 0) {
    assert.ok(hardDeleteIdx < earlyCleanup, 'hard delete must not be skipped by cleanup_pending');
  }
  // Hard delete must preserve billing aggregates.
  assert.match(fn, /usage_stats_preserved/);
  assert.match(fn, /hard_deleted/);
});

test('host delete also clears conversation-scoped tool_calls and artifacts', () => {
  const fn = host.slice(host.indexOf('async fn handle_conversation_delete'));
  assert.match(fn, /DELETE FROM assistant_tool_calls WHERE conversation_id/);
  assert.match(fn, /DELETE FROM assistant_artifacts WHERE conversation_id/);
});
