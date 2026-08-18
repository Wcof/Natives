import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const read = (path: string) => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');

test('credential broker key resolver is not Renderer IPC', () => {
  const broker = read('src-tauri/src/credential_broker.rs');
  const invokeRegistry = read('src-tauri/src/lib.rs');

  assert.match(broker, /pub async fn credential_broker_resolve/);
  assert.doesNotMatch(broker, /#\[tauri::command\]\s*pub async fn credential_broker_resolve/);
  assert.doesNotMatch(invokeRegistry, /crate::credential_broker::credential_broker_resolve/);
});
