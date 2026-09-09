import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { appFixture, CORE_HOST, ORIGIN } from './native-fixture.mjs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';

const bytes = readFileSync('dist/app-release/catalog-v2.json');
await verifyCatalogSignature({ catalogBytes: bytes, signatureB64: readFileSync('dist/app-release/catalog-v2.sig', 'utf8') });
const entry = JSON.parse(bytes).apps[0];
const fixture = appFixture();
try {
  const core = fixture.connect(CORE_HOST);
  const host = await core.call('apps:handshake', { origin: ORIGIN });
  assert.equal(host.appsProtocolVersion, 3);
  const packages = entry.packages.filter((pkg) => pkg.platform === 'any' && pkg.arch === 'any');
  const request = { app: { app_id: entry.app_id, kind: entry.kind, name: entry.name, version: entry.version,
    enabled: true, show_in_sidebar: true, runtime_spec: entry.runtime_spec, surface: entry.surface, manifest: entry.manifest },
    packages: packages.map(({ url, ...metadata }) => metadata), permissions: entry.permissions,
    min_host_version: entry.minHostVersion };
  const tx = await core.call('apps:install_begin', { request: Buffer.from(JSON.stringify(request)).toString('base64') });
  for (const pkg of packages) {
    const nap = readFileSync(join('dist/app-release', new URL(pkg.url).pathname.split('/').at(-1)));
    assert.equal(createHash('sha256').update(nap).digest('hex'), pkg.artifact_sha256);
    const payload = gunzipSync(nap);
    await core.call('apps:install_package', { installId: tx.install_id, packageId: pkg.package_id, data: payload.toString('base64') });
  }
  const installed = await core.call('apps:install_commit', { installId: tx.install_id });
  assert.equal(installed.host_registered, false);
  for (const pkg of packages) {
    const resource = await core.call('apps:read_resource', { appId: entry.app_id, packageId: pkg.package_id, offset: 0, length: 524288 });
    const payload = gunzipSync(readFileSync(join('dist/app-release', new URL(pkg.url).pathname.split('/').at(-1))));
    assert.deepEqual(Buffer.from(resource.data, 'base64'), payload);
    assert.equal(resource.version, entry.version);
  }
  const personal = join(fixture.root, 'apps', entry.app_id, 'data');
  mkdirSync(personal, { recursive: true }); writeFileSync(join(personal, 'notes'), 'preserved');
  await core.call('apps:uninstall', { appId: entry.app_id });
  assert.ok(existsSync(join(personal, 'notes')));
  await assert.rejects(core.call('apps:clear_data', { appId: entry.app_id }), /APP_CONFIRMATION_REQUIRED/);
  await core.call('apps:clear_data', { appId: entry.app_id, confirmPurge: true });
  assert.ok(!existsSync(personal));
  console.log(JSON.stringify({ realNativeMessaging: true, version: entry.version, install: 'passed', resources: packages.length, uninstallAndPurge: 'passed' }));
} finally { await fixture.dispose(); }
