// Development-only standard host package. It is deliberately unreleasable:
// the Core accepts this fixture only in debug builds and the publish gate
// refuses both fixture catalogs and missing platform-signature evidence.
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { join, resolve } from 'node:path';
import { ROOT } from '../extension-package.mjs';
import { RELEASE_MANIFEST } from './publish-release.mjs';

const version = process.env.NATIVES_SAMPLE_VERSION || '1.0.0';
const appId = process.env.NATIVES_SAMPLE_APP_ID || 'sample';
if (!/^[a-z0-9][a-z0-9._-]{0,63}$/.test(appId) || !/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error('invalid sample app id or version');
}
const installableFixture = process.argv.includes('--installable-fixture');
const outputDir = resolve(ROOT, process.argv.slice(2).find((arg) => !arg.startsWith('-')) || 'dist/app-release');
const binary = resolve(ROOT, 'target/debug/sample-host');
if (!existsSync(binary)) throw new Error('build sample-host-fixture before packaging');
// Ad-hoc sign the fixture payload BEFORE hashing (contract §3: sign first,
// then compute artifact/payload hashes). Unsigned Mach-O binaries trigger the
// slow unsigned-syspolicyd assessment path on macOS and can stall the
// bounded --health probe when spawned from the Chrome process tree. This is
// development-fixture-only; release payloads require real platform identity.
if (process.platform === 'darwin') {
  const { spawnSync } = await import('node:child_process');
  const signed = spawnSync('codesign', ['--force', '--sign', '-', binary], { encoding: 'utf8' });
  if (signed.status !== 0) throw new Error(`ad-hoc codesign failed: ${signed.stderr}`);
}
const payload = readFileSync(binary);
const artifact = gzipSync(payload, { level: 9 });
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const filename = `${appId}-host-${version}-darwin-arm64.nap`;
mkdirSync(outputDir, { recursive: true });
writeFileSync(join(outputDir, filename), artifact);

const extensionVersion = JSON.parse(readFileSync(resolve(ROOT, 'extension/manifest.json'), 'utf8')).version;
const catalog = {
  catalogVersion: 3,
  publishedAt: new Date(Number(process.env.SOURCE_DATE_EPOCH || Date.now() / 1000) * 1000).toISOString(),
  apps: [{
    app_id: appId, kind: 'managed_local', name: `Managed Sample ${appId}`, version,
    appProtocolVersion: 1, minExtensionVersion: extensionVersion, minHostVersion: '0.1.0',
    description: { zh_CN: '独立托管应用标准样例（开发 fixture）', en: 'Independent managed app sample (development fixture)' },
    icon: 'grid', permissions: ['app.lifecycle'], runtime_spec: {},
    surface: { icon: 'grid', route: `app.html?app=${encodeURIComponent(appId)}` },
    manifest: { schemaVersion: 1, fixture: true },
    packages: [{
      package_id: 'app-exec', kind: 'managed_local', version,
      platform: 'darwin', arch: 'arm64', required: true,
      url: `https://github.com/Wcof/Natives/releases/download/apps-${appId}-v${version}/${filename}`,
      wire_size: artifact.length, payload_size: payload.length,
      artifact_sha256: digest(artifact), payload_sha256: digest(payload),
    }],
    published: installableFixture,
  }],
};
const catalogBytes = Buffer.from(JSON.stringify(catalog, null, 2) + '\n');
writeFileSync(join(outputDir, 'catalog-v3.json'), catalogBytes);
// The catalog must be SIGNED with the compiled dev trust root (contract §2);
// an unsigned or mismatched catalog is rejected by the page and the Host.
import { signCatalog } from './catalog-signing.mjs';
import { join as _join } from 'node:path';
signCatalog(join(outputDir, 'catalog-v3.json'), _join(ROOT, 'scripts/apps/keys/catalog-trust-dev.private.pem'));
writeFileSync(join(outputDir, RELEASE_MANIFEST), JSON.stringify({
  schemaVersion: 1,
  candidate: 'development-fixture',
  catalog_sha256: digest(catalogBytes),
  packages: [{ name: filename, artifact_sha256: digest(artifact), payload_sha256: digest(payload) }],
}, null, 2) + '\n');
console.log(`generated unsigned development fixture in ${outputDir}; it cannot be published`);
