import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compareAppVersions } from '../../extension/app-catalog-policy.js';

export function checkCatalog(catalog) {
  assert.equal(catalog.catalogVersion, 1);
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
    assert.equal(entry.kind, 'extension_app');
    assert.ok(typeof entry.name === 'string' && entry.name.length > 0 && entry.name.length <= 128);
    assert.equal(compareAppVersions(entry.version, entry.version), 0, 'V1 catalog uses stable SemVer');
    const namespace = entry.app_id.startsWith('com.natives.app.') ? entry.app_id : `com.natives.app.${entry.app_id}`;
    assert.equal(entry.runtime_spec.host, namespace);
    assert.equal(entry.surface.route, `app.html?app=${encodeURIComponent(entry.app_id)}`);
    assert.ok(Array.isArray(entry.permissions) && entry.permissions.length <= 16);
    assert.ok(entry.permissions.every((permission) => ['app.lifecycle', `keychain:${namespace}`].includes(permission)));
    assert.ok(Array.isArray(entry.packages));
    noExecutableFields(entry);
  }
}
if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  checkCatalog(JSON.parse(readFileSync(new URL('../../extension/apps/catalog-v1.json', import.meta.url))));
  console.log('app manifest contract passed');
}
