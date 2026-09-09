import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolveAppPackages } from '../../extension/app-catalog-policy.js';

const extensionVersion = JSON.parse(readFileSync(new URL('../../extension/manifest.json', import.meta.url))).version;

export function checkPackageBudget(catalog) {
  for (const entry of catalog.apps) {
    if (!entry.packages || !entry.packages.length) continue;
    const resolved = resolveAppPackages({ ...entry, published: true }, { platform: 'any', arch: 'any', version: entry.minHostVersion, appsProtocolVersion: 3 }, extensionVersion);
    assert.equal(resolved.reason, null, `invalid package set for ${entry.app_id}`);
  }
}
if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const catalogPath = process.argv[2] ? resolve(process.argv[2]) : new URL('../../extension/apps/catalog-v2.json', import.meta.url);
  checkPackageBudget(JSON.parse(readFileSync(catalogPath)));
  console.log('app package budgets passed');
}
