import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';

const root = resolve(new URL('../..', import.meta.url).pathname);
const fundDir = resolve(root, '../Natives-App-Fund');
const seedsDist = join(root, 'dist', 'seeds');

mkdirSync(seedsDist, { recursive: true });

// Check or build fund package
console.log('Building Fund package in Natives-App-Fund...');
execFileSync('./scripts/package.sh', [], { cwd: fundDir, stdio: 'inherit' });

// Find the built .nap and .nap.meta.json
const fundCargo = readFileSync(join(fundDir, 'Cargo.toml'), 'utf8');
const versionMatch = fundCargo.match(/version\s*=\s*"([^"]+)"/);
const version = versionMatch ? versionMatch[1] : '0.1.0';

// Locate .nap file in dist
const triple = execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
  .split('\n')
  .find((l) => l.startsWith('host:'))
  .split(':')[1]
  .trim();

const napName = `fund-${version}-${triple}.nap`;
const napPath = join(fundDir, 'dist', napName);
if (!existsSync(napPath)) {
  throw new Error(`Expected Fund .nap at ${napPath}`);
}

const napBytes = readFileSync(napPath);
const wireSize = napBytes.length;
const artifactSha256 = createHash('sha256').update(napBytes).digest('hex');

// Read metadata
const metaPath = `${napPath}.meta.json`;
let payloadSize = 0;
let payloadSha256 = '';
if (existsSync(metaPath)) {
  const meta = JSON.parse(readFileSync(metaPath, 'utf8'));
  payloadSize = meta.payload_size;
  payloadSha256 = meta.payload_sha256;
} else {
  throw new Error(`Missing metadata file: ${metaPath}`);
}

// Copy to dist/seeds/fund.nap
const targetNapPath = join(seedsDist, 'fund.nap');
copyFileSync(napPath, targetNapPath);
console.log(`Copied ${napName} -> ${targetNapPath}`);

// Generate the SIGNED suite manifest (AC-03). Each app entry carries a full
// Ed25519-signed Catalog v3 produced by the app's own release pipeline plus
// the on-disk artifact name — no self-declared hashes. Core verifies every
// entry through the same trust chain as online installs.
const catalogPath = join(fundDir, 'dist', 'catalogs', 'fund.catalog.json');
const signaturePath = catalogPath.replace(/\.json$/, '.sig');
if (!existsSync(catalogPath) || !existsSync(signaturePath)) {
  throw new Error(
    `Missing signed catalog for fund: expected ${catalogPath} and ${signaturePath}. ` +
      'The app release pipeline must publish the signed Catalog v3 before the suite can be assembled.',
  );
}
const manifest = {
  schemaVersion: 2,
  suiteId: 'natives-suite',
  version,
  apps: [
    {
      app_id: 'fund',
      catalog_base64: readFileSync(catalogPath).toString('base64'),
      signature_base64: readFileSync(signaturePath).toString('ascii').trim(),
      artifact: 'fund.nap',
    },
  ],
};

const manifestPath = join(seedsDist, 'suite-manifest.json');
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
console.log(`Wrote signed suite-manifest.json -> ${manifestPath}`);
