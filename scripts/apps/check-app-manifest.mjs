import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { compareAppVersions } from '../../extension/app-catalog-policy.js';

export function checkCatalog(catalog) {
  assert.equal(catalog.catalogVersion, 3);
  assert.ok(Array.isArray(catalog.apps) && catalog.apps.length <= 128);
  const ids = new Set();
  function noExecutableFields(value) {
    if (!value || typeof value !== 'object') return;
    for (const [key, child] of Object.entries(value)) {
      assert.ok(!['scriptUrl', 'moduleUrl', 'wasmUrl', 'command', 'args', 'installPath', 'targetPath', 'path'].includes(key), `forbidden catalog field: ${key}`);
      noExecutableFields(child);
    }
  }
  for (const entry of catalog.apps) {
    assert.match(entry.app_id, /^[a-z0-9][a-z0-9._-]{0,63}$/);
    assert.ok(!ids.has(entry.app_id)); ids.add(entry.app_id);
    assert.equal(entry.kind, 'managed_local');
    assert.equal(entry.appProtocolVersion, 1);
    assert.ok(typeof entry.name === 'string' && entry.name.length > 0 && entry.name.length <= 128);
    assert.equal(compareAppVersions(entry.version, entry.version), 0, 'Catalog uses stable SemVer');
    const namespace = entry.app_id.startsWith('com.natives.app.') ? entry.app_id : `com.natives.app.${entry.app_id}`;
    assert.ok(!entry.runtime_spec?.host, 'catalog must not declare runtime_spec.host');
    assert.equal(compareAppVersions(entry.minExtensionVersion, entry.minExtensionVersion), 0, 'minExtensionVersion is required');
    assert.equal(compareAppVersions(entry.minHostVersion, entry.minHostVersion), 0, 'minHostVersion is required');
    if (entry.packages) {
      assert.equal(entry.packages.filter((pkg) => pkg.kind === 'managed_local').length, 1, 'one executable is required');
    }
    assert.equal(entry.surface.route, `app.html?app=${encodeURIComponent(entry.app_id)}`);
    assert.ok(Array.isArray(entry.permissions) && entry.permissions.length <= 16);
    assert.ok(entry.permissions.every((permission) => ['app.lifecycle', `keychain:${namespace}`].includes(permission)));
    assert.ok(Array.isArray(entry.packages));
    noExecutableFields(entry);
  }
}
if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const catalogPath = process.argv[2] ? resolve(process.argv[2]) : new URL('../../extension/apps/catalog-v3.json', import.meta.url);
  checkCatalog(JSON.parse(readFileSync(catalogPath)));
  console.log('app manifest contract passed');
}
