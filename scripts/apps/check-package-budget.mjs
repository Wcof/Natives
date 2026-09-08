import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolveAppPackages } from '../../extension/app-catalog-policy.js';

export function checkPackageBudget(catalog) {
  for (const entry of catalog.apps) {
    if (!entry.packages.length) continue;
    for (const runtime of entry.packages.filter((pkg) => pkg.kind === 'runtime')) {
      const resolved = resolveAppPackages({ ...entry, published: true }, { platform: runtime.platform, arch: runtime.arch, version: '0.1.0' });
      assert.equal(resolved.reason, null, `invalid package set for ${entry.app_id}`);
    }
    assert.ok(entry.packages.some((pkg) => pkg.kind === 'runtime'), 'package set must include a runtime');
  }
}
if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  checkPackageBudget(JSON.parse(readFileSync(new URL('../../extension/apps/catalog-v1.json', import.meta.url))));
  console.log('app package budgets passed');
}
