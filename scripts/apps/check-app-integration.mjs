import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { appFixture, CORE_HOST, ORIGIN } from './native-fixture.mjs';
import { signCatalog } from './catalog-signing.mjs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';

const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const candidates = mkdtempSync(join(tmpdir(), 'natives-a5-candidates-'));
const signingKey = resolve('scripts/apps/keys/catalog-trust-dev.private.pem');

function buildCandidate(appId, version) {
  const directory = join(candidates, `${appId}-${version}`);
  execFileSync(process.execPath, ['scripts/apps/package-demo.mjs', '--installable-fixture', directory], {
    cwd: resolve('.'), stdio: 'ignore',
    env: { ...process.env, NATIVES_SAMPLE_APP_ID: appId, NATIVES_SAMPLE_VERSION: version },
  });
  const catalogPath = join(directory, 'catalog-v3.json');
  signCatalog(catalogPath, signingKey);
  return directory;
}

async function readCandidate(directory) {
  const bytes = readFileSync(join(directory, 'catalog-v3.json'));
  const signature = readFileSync(join(directory, 'catalog-v3.sig'), 'utf8');
  await verifyCatalogSignature({ catalogBytes: bytes, signatureB64: signature });
  return { bytes, signature, entry: JSON.parse(bytes).apps[0], directory };
}

async function install(core, candidate) {
  const { bytes, signature, entry, directory } = candidate;
  const tx = await core.call('apps:install_begin', {
    appId: entry.app_id, catalogBase64: bytes.toString('base64'), signature,
  });
  const pkg = entry.packages.find((item) => item.package_id === tx.package_id);
  const artifact = readFileSync(join(directory, new URL(pkg.url).pathname.split('/').at(-1)));
  for (let offset = 0; offset < artifact.length; offset += tx.chunk_size) {
    const chunk = artifact.subarray(offset, Math.min(offset + tx.chunk_size, artifact.length));
    await core.call('apps:install_chunk', {
      installId: tx.install_id, packageId: pkg.package_id, offset,
      dataBase64: chunk.toString('base64'), chunkSha256: digest(chunk),
    });
  }
  await core.call('apps:install_finish', {
    installId: tx.install_id, packageId: pkg.package_id, artifactBytes: artifact.length,
  });
  return core.call('apps:install_commit', { installId: tx.install_id });
}

async function startApp(fixture, app, expectedVersion, value) {
  const port = fixture.connect(app.runtime_host);
  const handshake = await port.call('app:handshake', { protocolVersion: 1, expectedAppId: app.app_id });
  assert.equal(handshake.appVersion, expectedVersion);
  const started = await port.call('app:start', { requestId: `start-${expectedVersion}` });
  const session = await port.call('app:session', {
    instanceId: started.instanceId, op: 'issue', challenge: `challenge-${expectedVersion}`,
  });
  const options = { headers: { Authorization: `Bearer ${session.token}`, Origin: 'null' } };
  if (value !== undefined) {
    const saved = await fetch(`http://127.0.0.1:${started.port}/api/value`, {
      ...options, method: 'POST', body: value,
    });
    assert.ok(saved.ok);
  }
  const data = await fetch(`http://127.0.0.1:${started.port}/api/value`, options)
    .then((response) => response.json());
  await port.close();
  return data.value;
}

const coreHashBefore = digest(readFileSync('target/debug/native-file-host'));
const extensionFiles = ['extension/app.js', 'extension/apps.js', 'extension/manifest.json'];
const extensionDigest = () => digest(Buffer.concat(extensionFiles.map((path) => readFileSync(path))));
const extensionHashBefore = extensionDigest();
const fixture = appFixture();
try {
  const [v1, v2, fresh] = await Promise.all([
    readCandidate(buildCandidate('sample', '1.0.0')),
    readCandidate(buildCandidate('sample', '2.0.0')),
    readCandidate(buildCandidate('sample-new', '1.0.0')),
  ]);
  const core = fixture.connect(CORE_HOST);
  const host = await core.call('apps:handshake', { origin: ORIGIN });
  assert.equal(host.appsProtocolVersion, 4);

  const installedV1 = await install(core, v1);
  assert.equal(installedV1.kind, 'managed_local');
  assert.equal(installedV1.host_registered, true);
  assert.equal(await startApp(fixture, installedV1, '1.0.0', 'kept across update'), 'kept across update');

  const installedV2 = await install(core, v2);
  assert.equal(installedV2.version, '2.0.0');
  assert.equal(await startApp(fixture, installedV2, '2.0.0'), 'kept across update');

  // Verify apps:rollback restores previous version (contract §4.1)
  const rolledBack = await core.call('apps:rollback', { appId: 'sample' });
  assert.equal(rolledBack.app.version, '1.0.0');
  assert.equal(await startApp(fixture, rolledBack.app, '1.0.0'), 'kept across update');

  const installedFresh = await install(core, fresh);
  assert.equal(installedFresh.app_id, 'sample-new');
  assert.equal(await startApp(fixture, installedFresh, '1.0.0', 'new app works'), 'new app works');

  // Verify Core disabled state blocks App Host startup via activation.json projection (contract §3.1)
  await core.call('apps:set_enabled', { appId: 'sample-new', enabled: false });
  await assert.rejects(
    startApp(fixture, installedFresh, '1.0.0'),
    /APP_INSTALLATION_CHANGED/,
    'disabled app must be refused by App Host'
  );
  await core.call('apps:set_enabled', { appId: 'sample-new', enabled: true });
  assert.equal(await startApp(fixture, installedFresh, '1.0.0'), 'new app works');

  await core.call('apps:uninstall', { appId: 'sample' });
  assert.equal(readFileSync(join(fixture.root, 'apps', 'sample', 'data', 'value.txt'), 'utf8'), 'kept across update');
  await assert.rejects(core.call('apps:clear_data', { appId: 'sample' }), /APP_CONFIRMATION_REQUIRED/);
  await core.call('apps:clear_data', { appId: 'sample', confirmPurge: true });

  assert.equal(digest(readFileSync('target/debug/native-file-host')), coreHashBefore);
  assert.equal(extensionDigest(), extensionHashBefore);
  console.log(JSON.stringify({
    nativeMessaging: true, catalog: 3, coreProtocol: 4, appProtocol: 1,
    installV1: 'passed', updateV2: 'passed', newAppId: 'passed',
    fixedCoreHash: coreHashBefore, fixedExtensionHash: extensionHashBefore,
    registration: 'passed', dataProtection: 'passed',
  }));
} finally {
  await fixture.dispose();
  rmSync(candidates, { recursive: true, force: true });
}
