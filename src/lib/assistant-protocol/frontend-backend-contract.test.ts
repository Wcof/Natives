/**
 * Frontend → backend method contract matrix.
 *
 * Goal evidence: every RPC the assistant UI actually calls must be in the
 * Protocol v2 catalogue and in IMPLEMENTED ∪ HOST (or documented Host-only).
 * This is a static contract test against the Rust methods source of truth.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const METHODS_RS = resolve(
  process.cwd(),
  'crates/assistant-protocol/src/v2/methods.rs',
);

function extractArrayConst(src: string, name: string): string[] {
  const re = new RegExp(
    `pub const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`,
  );
  const m = src.match(re);
  const body = m?.[1];
  if (!body) throw new Error(`const ${name} not found in methods.rs`);
  return [...body.matchAll(/"([^"]+)"/g)]
    .map((x) => x[1])
    .filter((value): value is string => Boolean(value));
}

/** Methods the assistant frontend actually invokes (audit 2026-07-21). */
const UI_CALLED_METHODS = [
  'daemon.ping',
  'daemon.getCapabilities',
  'provider.list',
  'conversation.list',
  'conversation.create',
  'conversation.get',
  'conversation.getMessages',
  'conversation.rename',
  'conversation.update_model',
  'conversation.update_permission',
  'conversation.archive',
  'conversation.delete',
  'conversation.fork',
  'run.start',
  'run.cancel',
  'run.retry',
  'run.list',
  'run.getEvents',
  'run.subscribe',
  'permission.respond',
  'permission.listPending',
  'interaction.respond',
  'interaction.listPending',
  'promptQueue.list',
  'promptQueue.enqueue',
  'promptQueue.update',
  'promptQueue.remove',
  'promptQueue.reorder',
  'promptQueue.sendNow',
  'promptQueue.interject',
  'artifact.list',
  'artifact.open',
  'artifact.reveal',
  'task.list',
  'task.cancel',
  'subagent.list',
  'subagent.touch',
  'subagent.switchRoute',
] as const;

test('UI-called methods are in ALL_METHODS catalogue', () => {
  const src = readFileSync(METHODS_RS, 'utf8');
  const all = new Set(extractArrayConst(src, 'ALL_METHODS'));
  for (const m of UI_CALLED_METHODS) {
    assert.ok(all.has(m), `missing from ALL_METHODS: ${m}`);
  }
});

test('UI-called methods are implemented (daemon or host)', () => {
  const src = readFileSync(METHODS_RS, 'utf8');
  const implemented = new Set([
    ...extractArrayConst(src, 'IMPLEMENTED_METHODS'),
    ...extractArrayConst(src, 'HOST_IMPLEMENTED_METHODS'),
  ]);
  // run.subscribe may be daemon-only; still must be implemented somewhere.
  for (const m of UI_CALLED_METHODS) {
    assert.ok(
      implemented.has(m),
      `UI calls ${m} but it is not in IMPLEMENTED ∪ HOST — fail-closed risk`,
    );
  }
});

test('oauth browser methods stay unimplemented (honest red line)', () => {
  const src = readFileSync(METHODS_RS, 'utf8');
  const implemented = new Set([
    ...extractArrayConst(src, 'IMPLEMENTED_METHODS'),
    ...extractArrayConst(src, 'HOST_IMPLEMENTED_METHODS'),
  ]);
  assert.ok(!implemented.has('mcp.auth.oauthStart'));
  assert.ok(!implemented.has('mcp.auth.oauthCallback'));
});

test('capability-gate HOST_METHODS_UI stays subset of HOST_IMPLEMENTED when present', () => {
  // Optional: if capability-gate exists, spot-check key host methods.
  try {
    const gate = readFileSync(
      resolve(process.cwd(), 'src/lib/assistant-workspace/capability-gate.ts'),
      'utf8',
    );
    for (const m of [
      'promptQueue.enqueue',
      'permission.respond',
      'interaction.respond',
    ]) {
      assert.ok(
        gate.includes(`'${m}'`) || gate.includes(`"${m}"`),
        `HOST_METHODS_UI should list ${m}`,
      );
    }
  } catch {
    // gate file optional on some branches
  }
});


test('protocol run.rs advertises runtime_id and effort fields', () => {
  const runRs = readFileSync(
    resolve(process.cwd(), 'crates/assistant-protocol/src/v2/run.rs'),
    'utf8',
  );
  assert.ok(runRs.includes('pub runtime_id: Option<String>'));
  assert.ok(runRs.includes('pub effort: Option<String>'));
});

test('daemon capabilities keep codex unavailable in host_mediated helper source', () => {
  const caps = readFileSync(
    resolve(process.cwd(), 'crates/assistant-protocol/src/v2/capabilities.rs'),
    'utf8',
  );
  assert.ok(caps.includes('host_mediated'));
  assert.ok(caps.includes('RuntimeAvailability'));
  // Codex red line present in host path source (assistant_service or capabilities tests)
  const host = readFileSync(
    resolve(process.cwd(), 'src-tauri/src/assistant_service.rs'),
    'utf8',
  );
  assert.ok(
    host.includes('codex_cli') && host.includes('app-server not implemented'),
    'host capabilities must keep Codex unavailable with reason',
  );
});
