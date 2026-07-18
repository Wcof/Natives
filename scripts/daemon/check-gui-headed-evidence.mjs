#!/usr/bin/env node
import { readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';

const dir = process.argv[2];
if (!dir) {
  console.error('usage: check-gui-headed-evidence.mjs EVIDENCE_DIR');
  process.exit(2);
}

const evidenceDir = resolve(dir);
const mode = statSync(evidenceDir).mode & 0o777;
if (mode !== 0o700) {
  console.error(`evidence dir must be 0700: ${evidenceDir} mode=${mode.toString(8)}`);
  process.exit(1);
}

const raw = readFileSync(join(evidenceDir, 'gui-headed-evidence.json'), 'utf8');
if (/(sk-[A-Za-z0-9_-]{8,}|Bearer\s+[^\s"]+|api[_-]?key["'=:\s]+[A-Za-z0-9_-]{8,})/i.test(raw)) {
  console.error('evidence contains likely credential material');
  process.exit(1);
}

const doc = JSON.parse(raw);
const cases = doc.cases ?? {};
const required = [
  'provider_openai_compatible_created_and_tested',
  'provider_anthropic_created_and_tested',
  'project_path_three_turn_conversation',
  'ask_approved_once',
  'ask_denied_once',
  'full_access_write_without_ask',
  'cancel_during_generation',
  'retry_creates_new_run',
  'cross_provider_subagent',
  'daemon_kill_reconnect',
  'app_restart_replays_messages_events',
  'no_fixture_fake_provider_or_plaintext_key',
];

const missing = required.filter((name) => {
  const value = cases[name];
  return value !== true && value !== 'pass' && value?.status !== 'pass';
});

if (missing.length) {
  console.error(`missing/failed headed cases: ${missing.join(', ')}`);
  process.exit(1);
}

console.log(JSON.stringify({ status: 'pass', evidence_dir: evidenceDir, cases: required.length }));
