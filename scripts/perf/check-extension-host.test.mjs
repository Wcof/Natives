import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runExtensionBundleCheck } from './check-extension-bundle.mjs';
import { chmodSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { runNativeHostCheck } from './check-native-host.mjs';
import { checkInstallerSize, DEFAULT_BUDGET } from './check-installer-size.mjs';
import { spawnSync } from 'node:child_process';

test('extension gate excludes development garbage and passes a small package', () => {
  const root = mkdtempSync(join(tmpdir(), 'natives-extension-perf-'));
  try {
    mkdirSync(join(root, 'extension', 'node_modules'), { recursive: true });
    writeFileSync(join(root, 'extension', 'manifest.json'), '{}');
    writeFileSync(join(root, 'extension', 'files.js'), 'console.log(1)');
    mkdirSync(join(root, 'extension', '_locales', 'zh_CN'), { recursive: true });
    writeFileSync(join(root, 'extension', '_locales', 'zh_CN', 'messages.json'), '{}');
    writeFileSync(join(root, 'extension', 'README.md'), 'dev');
    writeFileSync(join(root, 'extension', 'node_modules', 'junk.js'), Buffer.alloc(500000));
    const result = runExtensionBundleCheck(root);
    assert.equal(result.ok, true);
    assert.deepEqual(result.files, ['_locales/zh_CN/messages.json', 'files.js', 'manifest.json']);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('extension gate fails when estimated package exceeds budget', () => {
  const root = mkdtempSync(join(tmpdir(), 'natives-extension-over-'));
  try {
    mkdirSync(join(root, 'extension'), { recursive: true });
    writeFileSync(join(root, 'extension', 'manifest.json'), '{}');
    writeFileSync(join(root, 'extension', 'files.js'), randomBytes(300000));
    assert.equal(runExtensionBundleCheck(root).ok, false);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('native host gate verifies roots once and EOF exit', async () => {
  const root = mkdtempSync(join(tmpdir(), 'natives-host-perf-'));
  try {
    const bin = join(root, 'host');
    writeFileSync(bin, `#!/usr/bin/env node
let b=Buffer.alloc(0);
process.stdin.on('data', c => {
  b=Buffer.concat([b,c]);
  while(b.length >= 4) {
    const l=b.readUInt32LE(0);
    if(b.length < l+4) break;
    const msg=JSON.parse(b.subarray(4, l+4).toString());
    b=b.subarray(l+4);
    const result = msg.method === 'roots' ? { roots: [] } : { entries: [], hasMore: false };
    const out=Buffer.from(JSON.stringify({ id: msg.id, ok: true, result }));
    const h=Buffer.alloc(4);
    h.writeUInt32LE(out.length);
    process.stdout.write(Buffer.concat([h, out]));
  }
});
process.stdin.on('end', () => process.exit(0));\n`);
    chmodSync(bin, 0o755);
    const result = await runNativeHostCheck(root, 'host');
    assert.equal(result.roots.responseOk, true);
    assert.equal(result.eof.ok, true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('installer gate fails missing and oversized artifacts', () => {
  const root = mkdtempSync(join(tmpdir(), 'natives-installer-perf-'));
  try {
    assert.equal(checkInstallerSize(join(root, 'missing')).ok, false);
    const artifact = join(root, 'installer.pkg');
    writeFileSync(artifact, Buffer.alloc(1024));
    assert.equal(checkInstallerSize(artifact).ok, true);
    assert.equal(checkInstallerSize(artifact, 512).ok, false);
    assert.equal(checkInstallerSize(artifact, Number.NaN).ok, false);
    assert.equal(DEFAULT_BUDGET, 10 * 1024 * 1024);
    const cli = spawnSync(process.execPath, ['scripts/perf/check-installer-size.mjs', artifact, 'not-a-budget'], { encoding: 'utf8' });
    assert.notEqual(cli.status, 0);
    const validCli = spawnSync(process.execPath, ['scripts/perf/check-installer-size.mjs', artifact, '2048'], { encoding: 'utf8' });
    assert.equal(validCli.status, 0);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
