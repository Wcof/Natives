import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { checkCatalog } from './check-app-manifest.mjs';
import { checkPackageBudget } from './check-package-budget.mjs';
import { assertPublishableCatalog } from './publish-release.mjs';
import { distributableFiles } from '../extension-package.mjs';

const catalog = JSON.parse(readFileSync(new URL('../../extension/apps/catalog-v3.json', import.meta.url)));

test('Catalog v3 fixture passes contract and budget gates but cannot be released', () => {
  checkCatalog(catalog);
  checkPackageBudget(catalog);
  assert.throws(() => assertPublishableCatalog(catalog), /fixture|published/i);
});

test('release catalog requires signed managed applications, not development fixtures', () => {
  const release = structuredClone(catalog);
  release.apps[0].manifest.fixture = false;
  release.apps[0].published = true;
  assert.doesNotThrow(() => assertPublishableCatalog(release));
});

test('extension ships metadata only, without managed executable payloads', () => {
  const files = distributableFiles();
  assert.ok(files.includes('apps/catalog-v3.json'));
  assert.ok(files.includes('apps/catalog-v3.sig'));
  assert.ok(!files.some((file) => /(?:app-module-registry|demo-ui|fund-ui)/.test(file)));
});
