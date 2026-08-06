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

/**
 * Drop `//` line comments so a quoted phrase inside one is never mistaken for
 * a method name. A `//` only starts a comment when it sits outside a string,
 * so count unescaped quotes ahead of it rather than cutting on first sight.
 */
function stripLineComments(block, quote) {
  return block
    .split('\n')
    .map((line) => {
      let inString = false;
      for (let i = 0; i < line.length; i += 1) {
        const ch = line[i];
        if (ch === '\\') {
          i += 1;
        } else if (ch === quote) {
          inString = !inString;
        } else if (!inString && ch === '/' && line[i + 1] === '/') {
          return line.slice(0, i);
        }
      }
      return line;
    })
    .join('\n');
}

/** Slice a `&[ ... ]` literal by balancing brackets, not by first `];`. */
function rustArray(name) {
  const header = methodsSrc.match(new RegExp(`pub const ${name}: &\\[&str\\] = &\\[`));
  if (!header) fail(`Rust ${name} catalogue missing`);
  let depth = 1;
  let i = header.index + header[0].length;
  const start = i;
  while (depth > 0) {
    if (i >= methodsSrc.length) fail(`Rust ${name} catalogue is unterminated`);
    if (methodsSrc[i] === '[') depth += 1;
    else if (methodsSrc[i] === ']') depth -= 1;
    i += 1;
  }
  const body = stripLineComments(methodsSrc.slice(start, i - 1), '"');
  return new Set([...body.matchAll(/"([^"]+)"/g)].map(([, value]) => value));
}

const rustMethods = rustArray('ALL_METHODS');
const daemonMethods = rustArray('IMPLEMENTED_METHODS');
const hostMethods = rustArray('HOST_IMPLEMENTED_METHODS');
const assistantMethodBlock = typesSrc.match(/export type AssistantMethod =([\s\S]*?);/);
if (!assistantMethodBlock) fail('TS AssistantMethod declaration missing');
const tsMethods = new Set(
  [...stripLineComments(assistantMethodBlock[1], "'").matchAll(/'([^']+)'/g)].map(
    ([, value]) => value,
  ),
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
  'generation_attempt_committed',
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

// Creative proposal field contract (T06): the wire payload must use the Rust
// serde(camelCase) name `environmentKeys`. A hand-written `envKeys` twin
// silently drops the field at runtime (Rust serializes one, TS reads the
// other) — the original bug this gate exists to prevent.
if (!typesSrc.includes('environmentKeys')) {
  fail('TS CreativeProposalPayload must declare environmentKeys (Rust serde camelCase)');
}
if (/\benvKeys\s*[:;,]/.test(typesSrc)) {
  fail('TS proposal types must use environmentKeys, never envKeys (field silently drops otherwise)');
}
if (!typesSrc.includes('proposalId')) {
  fail('TS CreativeProposalEnvelope must declare proposalId (stable Daemon-generated id)');
}

console.log('[protocol-sync] OK — TS types aligned with assistant-protocol core surface');
console.log(`[protocol-sync] Rust methods catalogued: ${rustMethods.size}`);
process.exit(0);
