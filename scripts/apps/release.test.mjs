import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { checkPackageBudget } from './check-package-budget.mjs';
import { checkCatalog } from './check-app-manifest.mjs';
import { signCatalog } from './catalog-signing.mjs';
import { buildExtension, distributableFiles, extensionFileBytes, ROOT } from '../extension-package.mjs';

const catalogFile = new URL('../../extension/apps/catalog-v2.json', import.meta.url);
const catalogJson = JSON.parse(readFileSync(catalogFile, 'utf8'));
const entry = catalogJson.apps[0];
const dataPkg = entry.packages[0];
const data = (id, bytes, required = false) => ({ ...dataPkg, package_id: id, kind: 'data', payload_size: bytes, required });

test('package gates apply to the aggregate platform set', () => {
  const check = (packages) => checkPackageBudget({ apps: [{ ...entry, packages }] });
  check([dataPkg, data('one', 20 * 1024 * 1024), data('two', 20 * 1024 * 1024)]);
  assert.throws(() => check([dataPkg, ...[1, 2, 3].map((id) => data(String(id), 20 * 1024 * 1024))]));
  assert.throws(() => check([dataPkg, ...[1, 2, 3].map((id) => data(String(id), 1, true))]));
  assert.throws(() => check([dataPkg, ...Array.from({ length: 16 }, (_, i) => data(String(i), 1))]));
  assert.throws(() => check([{ ...dataPkg, wire_size: 5 * 1024 * 1024 + 1 }]));
});

test('all distributors include App UI and the exact signed bytes, excluding native packages', () => {
  const root = mkdtempSync(join(tmpdir(), 'natives-package-input-'));
  const put = (name, content) => writeFileSync(join(root, 'extension', name), content);
  try {
    mkdirSync(join(root, 'extension/apps'), { recursive: true });
    put('manifest.json', '{ "manifest_version": 3 }');
    put('apps/demo-ui.js', 'export const included = true;');
    put('apps/catalog-v2.json', '{ "catalogVersion": 2 }\n');
    put('apps/catalog-v2.sig', 'exact signature bytes\n');
    put('apps.test.mjs', 'test only');
    const files = buildExtension(join(root, 'output'), root);
    assert.deepEqual(files, ['apps/catalog-v2.json', 'apps/catalog-v2.sig', 'apps/demo-ui.js', 'manifest.json']);
    for (const file of files) assert.deepEqual(readFileSync(join(root, 'output', file)), extensionFileBytes(file, root));
    assert.equal(readFileSync(join(root, 'output/apps/catalog-v2.json'), 'utf8'), '{ "catalogVersion": 2 }\n');
    put('apps/demo.nap', 'forbidden');
    assert.throws(() => distributableFiles(root), /Native executable/);
    rmSync(join(root, 'extension/apps/demo.nap'));
    put('apps/renamed.js', Buffer.from('7f454c4600000000', 'hex'));
    assert.throws(() => distributableFiles(root), /Native executable/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('clean checkout can generate a v2 candidate without a developer key or source rewrites', () => {
  const output = mkdtempSync(join(tmpdir(), 'natives-candidate-'));
  const sourceBefore = readFileSync(catalogFile);
  try {
    const env = { ...process.env };
    delete env.NATIVES_CATALOG_SIGNING_KEY;
    execFileSync(process.execPath, ['scripts/apps/package-demo.mjs', output], { cwd: ROOT, env });
    assert.deepEqual(readFileSync(catalogFile), sourceBefore, 'candidate generation must not rewrite extension sources');
    const candidatePath = join(output, 'catalog-v2.json');
    const candidate = JSON.parse(readFileSync(candidatePath));
    checkCatalog(candidate);
    checkPackageBudget(candidate);
    assert.deepEqual(candidate.apps[0].packages.map((pkg) => pkg.kind), ['data', 'resource']);
    assert.throws(() => signCatalog(candidatePath), /NATIVES_CATALOG_SIGNING_KEY/);
  } finally { rmSync(output, { recursive: true, force: true }); }
});
