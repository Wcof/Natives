#!/usr/bin/env node
/**
 * CI gate: ensure TS protocol surface stays aligned with Rust assistant-protocol.
 * Checks that required method names and event type strings exist in both trees.
 */
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const methodsRs = resolve(root, 'crates/assistant-protocol/src/v2/methods.rs');
const eventsRs = resolve(root, 'crates/assistant-protocol/src/v2/run_event.rs');
const typesTs = resolve(root, 'src/lib/assistant-protocol/types.ts');

function fail(msg) {
  console.error(`[protocol-sync] FAIL: ${msg}`);
  process.exit(1);
}

for (const f of [methodsRs, eventsRs, typesTs]) {
  if (!existsSync(f)) fail(`missing ${f}`);
}

const methodsSrc = readFileSync(methodsRs, 'utf8');
const eventsSrc = readFileSync(eventsRs, 'utf8');
const typesSrc = readFileSync(typesTs, 'utf8');

if (!typesSrc.includes('GENERATED-FROM: crates/assistant-protocol')) {
  fail('types.ts must declare GENERATED-FROM: crates/assistant-protocol');
}

function rustArray(name) {
  const match = methodsSrc.match(new RegExp(`pub const ${name}: &\\[&str\\] = &\\[(.*?)\\];`, 's'));
  if (!match) fail(`Rust ${name} catalogue missing`);
  return new Set([...match[1].matchAll(/"([^\"]+)"/g)].map(([, value]) => value));
}

const rustMethods = rustArray('ALL_METHODS');
const daemonMethods = rustArray('IMPLEMENTED_METHODS');
const hostMethods = rustArray('HOST_IMPLEMENTED_METHODS');
const assistantMethodBlock = typesSrc.match(/export type AssistantMethod =([\s\S]*?);/);
if (!assistantMethodBlock) fail('TS AssistantMethod declaration missing');
const tsMethods = new Set(
  [...assistantMethodBlock[1].matchAll(/'([^']+)'/g)].map(([, value]) => value),
);

for (const name of rustMethods) {
  if (!tsMethods.has(name)) fail(`TS AssistantMethod missing ${name}`);
}
for (const name of tsMethods) {
  if (!rustMethods.has(name)) fail(`Rust ALL_METHODS missing ${name}`);
}
for (const name of new Set([...daemonMethods, ...hostMethods])) {
  if (!rustMethods.has(name)) fail(`Implemented method missing from Rust ALL_METHODS: ${name}`);
}

const eventTypes = [
  'text_delta',
  'reasoning_delta',
  'tool_call_requested',
  'tool_call_completed',
  'permission_requested',
  'completed',
  'failed',
  'interrupted',
  'cancelled',
  'subagent_created',
  'generation_attempt_started',
  'generation_attempt_failed',
  'generation_attempt_discarded',
];
for (const t of eventTypes) {
  if (!eventsSrc.includes(`"${t}"`) && !eventsSrc.includes(`=> "${t}"`)) {
    // type_name uses snake_case strings
    if (!eventsSrc.includes(t)) fail(`Rust run_event missing ${t}`);
  }
  if (!typesSrc.includes(`'${t}'`)) fail(`TS RunEventType missing ${t}`);
}

// Core status enums
for (const s of ['waiting_permission', 'waiting_subagent', 'cancelling', 'interrupted']) {
  if (!typesSrc.includes(`'${s}'`)) fail(`TS RunStatus missing ${s}`);
}

console.log('[protocol-sync] OK — TS types aligned with assistant-protocol core surface');
console.log(`[protocol-sync] Rust methods catalogued: ${rustMethods.size}`);
process.exit(0);
