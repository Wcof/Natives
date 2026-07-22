#!/usr/bin/env node
/**
 * Assistant ↔ Engine linkage evidence pack (automated half of E2E).
 * Runs contract + regression unit tests and cargo tests when available.
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

const root = process.cwd();
const results = [];

function run(label, cmd, args) {
  console.log(`\n==> ${label}\n$ ${cmd} ${args.join(' ')}`);
  const r = spawnSync(cmd, args, {
    cwd: root,
    encoding: 'utf8',
    env: process.env,
  });
  const ok = r.status === 0;
  results.push({ label, ok, status: r.status });
  if (r.stdout) process.stdout.write(r.stdout);
  if (r.stderr) process.stderr.write(r.stderr);
  if (!ok) console.error(`[FAIL] ${label} exit=${r.status}`);
  return ok;
}

const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
const cargo = 'cargo';

let all = true;
all =
  run('frontend-backend-contract', npx, [
    '--yes',
    'tsx',
    '--test',
    'src/lib/assistant-protocol/frontend-backend-contract.test.ts',
  ]) && all;
all =
  run('wire+linkage', npx, [
    '--yes',
    'tsx',
    '--test',
    'src/lib/assistant-protocol/wire.test.ts',
    'src/lib/assistant-workspace/linkage-regression.test.ts',
  ]) && all;
all =
  run('controller', npx, [
    '--yes',
    'tsx',
    '--test',
    'src/lib/assistant-workspace/controller.test.ts',
  ]) && all;
all =
  run('full-linkage-e2e', npx, [
    '--yes',
    'tsx',
    '--test',
    'src/lib/assistant-workspace/full-linkage.e2e.test.ts',
  ]) && all;

if (existsSync(resolve(root, 'Cargo.toml'))) {
  all =
    run('cli_runtime_bridge', cargo, [
      'test',
      '-p',
      'natives-agent-daemon',
      '--lib',
      'cli_runtime_bridge',
      '--',
      '--test-threads=1',
    ]) && all;
  all =
    run('codex-fail-closed', cargo, [
      'test',
      '-p',
      'natives-agent-daemon',
      '--lib',
      'run_manager::tests::start_detached_codex',
      '--',
      '--test-threads=1',
    ]) && all;
  all =
    run('codex-runtime-bridge', cargo, [
      'test',
      '-p',
      'natives-agent-daemon',
      '--lib',
      'codex_runtime_bridge',
      '--',
      '--test-threads=1',
    ]) && all;
  all =
    run('assistant-protocol', cargo, ['test', '-p', 'assistant-protocol', '--lib']) && all;
}

console.log('\n======== LINKAGE EVIDENCE SUMMARY ========');
for (const r of results) {
  console.log(`${r.ok ? 'PASS' : 'FAIL'}  ${r.label}`);
}
console.log(all ? '\nOVERALL: PASS' : '\nOVERALL: FAIL');
process.exit(all ? 0 : 1);
