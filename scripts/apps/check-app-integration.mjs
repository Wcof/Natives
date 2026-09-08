import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { appFixture, CORE_HOST, ORIGIN } from './native-fixture.mjs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';

const fixtureCatalog = process.argv.includes('--fixture-catalog');
const platform = process.platform === 'win32' ? 'windows' : process.platform;
let entry;
if (fixtureCatalog) {
  // CI exercises native installation with its freshly built artifact, without a release private key.
  entry = JSON.parse(readFileSync(`dist/app-release/catalog-${platform}-${process.arch}.json`));
} else {
  const bytes = readFileSync('dist/app-release/catalog-v1.json');
  await verifyCatalogSignature({ catalogBytes: bytes, signatureB64: readFileSync('dist/app-release/catalog-v1.sig', 'utf8') });
  entry = JSON.parse(bytes).apps[0];
}
const fixture = appFixture();
try {
  const core = fixture.connect(CORE_HOST);
  const host = await core.call('apps:handshake', { origin: ORIGIN });
  assert.equal(host.appsProtocolVersion, 2);
  const packages = entry.packages.filter((pkg) => pkg.platform === host.platform && pkg.arch === host.arch);
  assert.equal(packages.length, 1);
  const request = { app: { app_id: entry.app_id, kind: entry.kind, name: entry.name, version: entry.version,
    enabled: true, show_in_sidebar: true, runtime_spec: entry.runtime_spec, surface: entry.surface, manifest: entry.manifest },
    packages: packages.map(({ url, ...metadata }) => metadata), permissions: entry.permissions };
  const tx = await core.call('apps:install_begin', { request: Buffer.from(JSON.stringify(request)).toString('base64') });
  const packageInfo = packages[0];
  const nap = readFileSync(join('dist/app-release', new URL(packageInfo.url).pathname.split('/').at(-1)));
  assert.equal(createHash('sha256').update(nap).digest('hex'), packageInfo.artifact_sha256);
  const payload = gunzipSync(nap);
  await core.call('apps:install_package', { installId: tx.install_id, packageId: packageInfo.package_id, data: payload.toString('base64') });
  const installed = await core.call('apps:install_commit', { installId: tx.install_id });
  assert.ok(installed.host_registered);
  const runtime = fixture.connect(entry.runtime_spec.host);
  assert.deepEqual(await runtime.call('ping'), { pong: true });
  assert.equal((await runtime.call('version')).version, entry.version);
  // A live runtime's shared lock prevents destructive filesystem work.
  await assert.rejects(core.call('apps:uninstall', { appId: entry.app_id }), /APP_BUSY/);
  const eofMs = await runtime.close();
  assert.ok(eofMs < 2000);
  const personal = join(fixture.root, 'apps', entry.app_id, 'data');
  mkdirSync(personal, { recursive: true }); writeFileSync(join(personal, 'notes'), 'preserved');
  await core.call('apps:uninstall', { appId: entry.app_id });
  assert.ok(existsSync(join(personal, 'notes')));
  assert.equal((await core.call('apps:list')).retainedData.length, 1);
  await assert.rejects(core.call('apps:clear_data', { appId: entry.app_id }), /APP_CONFIRMATION_REQUIRED/);
  await core.call('apps:clear_data', { appId: entry.app_id, confirmPurge: true });
  assert.ok(!existsSync(personal));
  assert.equal((await core.call('apps:list')).apps.length, 0);
  console.log(JSON.stringify({ realNativeMessaging: true, fixtureCatalog, version: entry.version, artifactBytes: nap.length,
    install: 'passed', runtimePing: 'passed', activeRuntimeGuard: 'passed', eofMs: Math.round(eofMs), uninstallAndPurge: 'passed' }));
} finally { await fixture.dispose(); }
