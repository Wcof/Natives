import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkPackageBudget } from './check-package-budget.mjs';
import { buildExtension, distributableFiles, extensionFileBytes } from '../extension-package.mjs';

const entry = JSON.parse(readFileSync(new URL('../../extension/apps/catalog-v1.json', import.meta.url))).apps[0];
const runtime = entry.packages[0];
const data = (id, bytes, required = false) => ({ ...runtime, package_id: id, kind: 'data', payload_size: bytes, required });

test('package gates apply to the aggregate platform set', () => {
  const check = (packages) => checkPackageBudget({ apps: [{ ...entry, packages }] });
  check([runtime, data('one', 20 * 1024 * 1024), data('two', 20 * 1024 * 1024)]);
  assert.throws(() => check([runtime, ...[1, 2, 3].map((id) => data(String(id), 20 * 1024 * 1024))]));
  assert.throws(() => check([runtime, ...[1, 2, 3].map((id) => data(String(id), 1, true))]));
  assert.throws(() => check([runtime, ...Array.from({ length: 16 }, (_, i) => data(String(i), 1))]));
  assert.throws(() => check([runtime, { ...runtime, package_id: 'second-runtime' }]));
  assert.throws(() => check([{ ...runtime, wire_size: 5 * 1024 * 1024 + 1 }]));
});

test('all distributors include App UI and the exact signed bytes, excluding native packages', () => {
  const root = mkdtempSync(join(tmpdir(), 'natives-package-input-'));
  const put = (name, content) => writeFileSync(join(root, 'extension', name), content);
  try {
    mkdirSync(join(root, 'extension/apps'), { recursive: true });
    put('manifest.json', '{ "manifest_version": 3 }');
    put('apps/demo-ui.js', 'export const included = true;');
    put('apps/catalog-v1.json', '{ "catalogVersion": 1 }\n');
    put('apps/catalog-v1.sig', 'exact signature bytes\n');
    put('apps.test.mjs', 'test only');
    const files = buildExtension(join(root, 'output'), root);
    assert.deepEqual(files, ['apps/catalog-v1.json', 'apps/catalog-v1.sig', 'apps/demo-ui.js', 'manifest.json']);
    for (const file of files) assert.deepEqual(readFileSync(join(root, 'output', file)), extensionFileBytes(file, root));
    assert.equal(readFileSync(join(root, 'output/apps/catalog-v1.json'), 'utf8'), '{ "catalogVersion": 1 }\n');
    put('apps/demo.nap', 'forbidden');
    assert.throws(() => distributableFiles(root), /Native executable/);
    rmSync(join(root, 'extension/apps/demo.nap'));
    put('apps/renamed.js', Buffer.from('7f454c4600000000', 'hex'));
    assert.throws(() => distributableFiles(root), /Native executable/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
