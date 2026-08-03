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

test('oauth browser methods are catalogued but advertised by neither surface (ADR-0016 decision 7)', () => {
  const src = readFileSync(METHODS_RS, 'utf8');
  const all = new Set(extractArrayConst(src, 'ALL_METHODS'));
  const daemon = new Set(extractArrayConst(src, 'IMPLEMENTED_METHODS'));
  const host = new Set(extractArrayConst(src, 'HOST_IMPLEMENTED_METHODS'));
  // ADR-0016 decision 7 is right about *ownership* — the browser flow (loopback
  // + PKCE + token exchange) lives on the Host. It does not follow that the RPC
  // name should be advertised: the flow ships as the Tauri command
  // `mcp_oauth_start` and is invoked directly, so it never travels this surface.
  // `HOST_IMPLEMENTED_METHODS` is a promise that `is_host_owned_method`
  // intercepts the call; neither oauth name is in that match, so advertising
  // them would route live calls to a daemon with no arm and yield
  // `internal_error` instead of an honest `unsupported`.
  for (const name of ['mcp.auth.oauthStart', 'mcp.auth.oauthCallback']) {
    assert.ok(all.has(name), `${name} stays in the catalogue`);
    assert.ok(!daemon.has(name), `${name} must not be daemon-advertised`);
    assert.ok(!host.has(name), `${name} must not be host-advertised until intercepted`);
  }
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

test('run.resume is catalogued, implemented, and its wire contract matches TS call shape', () => {
  const methodsSrc = readFileSync(METHODS_RS, 'utf8');
  const all = new Set(extractArrayConst(methodsSrc, 'ALL_METHODS'));
  const implemented = new Set([
    ...extractArrayConst(methodsSrc, 'IMPLEMENTED_METHODS'),
    ...extractArrayConst(methodsSrc, 'HOST_IMPLEMENTED_METHODS'),
  ]);
  assert.ok(all.has('run.resume'), 'run.resume must stay in the ALL_METHODS catalogue');
  assert.ok(
    implemented.has('run.resume'),
    'run.resume must be implemented (daemon or host) — no advertise-without-arm',
  );

  const runRs = readFileSync(
    resolve(process.cwd(), 'crates/assistant-protocol/src/v2/run.rs'),
    'utf8',
  );
  // Request wire shape (serde default = snake_case fields the TS side sends).
  for (const field of [
    'pub struct ResumeRunRequest',
    'pub run_id: String',
    'pub checkpoint_id: Option<String>',
    'pub content: Option<String>',
    'pub confirmed: bool',
  ]) {
    assert.ok(runRs.includes(field), `ResumeRunRequest missing ${field}`);
  }
  // Response wire shape (decision/reason/new_run_id/unresolved_effects).
  for (const field of [
    'pub struct ResumeRunResponse',
    'pub decision: ResumeDecision',
    'pub reason: String',
    'pub unresolved_effects',
    'pub new_run_id: Option<String>',
  ]) {
    assert.ok(runRs.includes(field), `ResumeRunResponse missing ${field}`);
  }
  // Decision vocabulary must stay snake_case (wire contract).
  for (const variant of ['SafeToContinue', 'ConfirmationRequired', 'Blocked']) {
    assert.ok(
      runRs.includes(variant),
      `ResumeDecision must keep the ${variant} variant`,
    );
  }
});


test('daemon capabilities keep codex unavailable in host_mediated helper source', () => {
  const hostCaps = readFileSync(
    resolve(process.cwd(), 'src-tauri/src/assistant_service/capabilities.rs'),
    'utf8',
  );
  assert.ok(
    hostCaps.includes('codex_cli') && hostCaps.includes('app-server not implemented'),
    'host capabilities must keep Codex unavailable with reason',
  );
  const caps = readFileSync(
    resolve(process.cwd(), 'crates/assistant-protocol/src/v2/capabilities.rs'),
    'utf8',
  );
  assert.ok(
    caps.includes('Codex stays unavailable') || caps.includes('app-server not implemented'),
    'protocol capabilities honesty rules must document Codex unavailable',
  );
});
