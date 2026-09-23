import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

export function checkProductManifest(manifest) {
  assert.ok(manifest.schemaVersion === 1 || manifest.schemaVersion === 2, 'schemaVersion must be 1 or 2');
  assert.equal(manifest.product, 'natives');
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(['darwin', 'windows', 'linux'].includes(manifest.platform));
  assert.ok(['arm64', 'x64'].includes(manifest.arch));
  if (manifest.schemaVersion === 2) {
    assert.ok(manifest.appRuntime, 'appRuntime section required in schema 2');
    assert.equal(manifest.appRuntime.protocolVersion, 2);
    assert.ok(!manifest.appRuntime.path.startsWith('/') && !manifest.appRuntime.path.split('/').includes('..'));
    assert.match(manifest.appRuntime.sha256, /^[a-f0-9]{64}$/);
  }
  assert.ok(Array.isArray(manifest.modules) && manifest.modules.length > 0);
  const ids = new Set();
  for (const module of manifest.modules) {
    assert.match(module.appId, /^[a-z0-9][a-z0-9._-]{0,63}$/);
    assert.ok(!ids.has(module.appId), `duplicate module ${module.appId}`);
    ids.add(module.appId);
    assert.equal(module.entryRoute, `app.html?app=${encodeURIComponent(module.appId)}`);
    if (manifest.schemaVersion === 2) {
      assert.ok(typeof module.moduleApiVersion === 'number');
      assert.ok(typeof module.dataSchemaVersion === 'number');
      assert.ok(typeof module.capabilityVersion === 'number');
    } else {
      assert.match(module.version, /^\d+\.\d+\.\d+$/);
      assert.ok(!module.artifactPath.startsWith('/') && !module.artifactPath.split('/').includes('..'));
      assert.match(module.payloadSha256, /^[a-f0-9]{64}$/);
    }
  }
}

const input = process.argv[2];
const manifest = input
  ? JSON.parse(readFileSync(resolve(input), 'utf8'))
  : {
      schemaVersion: 2,
      product: 'natives',
      version: '0.1.0',
      platform: 'darwin',
      arch: 'arm64',
      appRuntime: {
        protocolVersion: 2,
        path: 'hosts/natives-app-runtime',
        bytes: 12345,
        sha256: 'a'.repeat(64),
      },
      modules: [{
        appId: 'fund',
        entryRoute: 'app.html?app=fund',
        moduleApiVersion: 1,
        dataSchemaVersion: 1,
        capabilityVersion: 1,
        ui: {
          path: 'modules/fund/ui',
          treeSha256: 'b'.repeat(64),
        },
      }],
    };
checkProductManifest(manifest);
console.log('product manifest contract passed');
