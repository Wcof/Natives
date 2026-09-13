// Publish only independently-delivered, signed managed-app candidates.
// This has no extension/Core build step: protocol-compatible app updates
// must not republish either product.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { basename, join, resolve } from 'node:path';
import { checkCatalog } from './check-app-manifest.mjs';
import { checkPackageBudget } from './check-package-budget.mjs';
import { verifyCatalogSignature, PACKAGE_MAX_PAYLOAD_BYTES } from '../../extension/catalog-client.js';

export const RELEASE_MANIFEST = 'release-manifest-v1.json';
const repository = 'Wcof/Natives';
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const platformVerifiers = new Set(['macos-codesign+gatekeeper', 'windows-authenticode', 'linux-ed25519-only']);

function assetName(pkg) {
  const url = new URL(pkg.url);
  assert.equal(url.protocol, 'https:', 'managed packages require HTTPS');
  assert.equal(url.hostname, 'github.com', 'managed packages require the fixed official release host');
  assert.equal(url.pathname.split('/').slice(1, 3).join('/'), repository, 'managed package repository changed');
  const name = basename(url.pathname);
  assert.match(name, /^[a-zA-Z0-9._-]+\.nap$/, 'invalid managed package filename');
  return name;
}

function proofName(name) { return `signatures/${name}.json`; }

export function assertPublishableCatalog(catalog) {
  checkCatalog(catalog);
  checkPackageBudget(catalog);
  assert.ok(catalog.apps.length, 'release catalog has no apps');
  for (const app of catalog.apps) {
    assert.equal(app.kind, 'managed_local');
    assert.equal(app.published, true, `${app.app_id}: unpublished entries cannot enter a release catalog`);
    assert.notEqual(app.manifest?.fixture, true, `${app.app_id}: isolated development fixtures cannot be published`);
    assert.ok(typeof app.publishedAt === 'string' || typeof catalog.publishedAt === 'string', `${app.app_id}: publishedAt is required`);
    for (const pkg of app.packages) assetName(pkg);
  }
}

function readJson(path) { return JSON.parse(readFileSync(path, 'utf8')); }

export async function verifyCandidate(directory = resolve('dist/app-release')) {
  const catalogPath = join(directory, 'catalog-v3.json');
  const signaturePath = join(directory, 'catalog-v3.sig');
  const manifestPath = join(directory, RELEASE_MANIFEST);
  assert.ok(existsSync(catalogPath), 'missing catalog-v3.json');
  assert.ok(existsSync(signaturePath), 'missing catalog-v3.sig');
  assert.ok(existsSync(manifestPath), `missing ${RELEASE_MANIFEST}`);
  const bytes = readFileSync(catalogPath);
  await verifyCatalogSignature({ catalogBytes: bytes, signatureB64: readFileSync(signaturePath, 'utf8') });
  const catalog = JSON.parse(bytes);
  assertPublishableCatalog(catalog);
  const releaseManifest = readJson(manifestPath);
  assert.equal(releaseManifest.schemaVersion, 1, 'unsupported release manifest');
  assert.equal(releaseManifest.catalog_sha256, digest(bytes), 'release manifest catalog hash mismatch');
  const releases = new Map();
  const inventory = new Map((releaseManifest.packages || []).map((item) => [item.name, item]));
  for (const app of catalog.apps) for (const pkg of app.packages) {
    const name = assetName(pkg), file = join(directory, name), nap = readFileSync(file);
    assert.equal(nap.length, pkg.wire_size, `${name}: wire size`);
    assert.equal(digest(nap), pkg.artifact_sha256, `${name}: artifact hash`);
    const payload = gunzipSync(nap, { maxOutputLength: PACKAGE_MAX_PAYLOAD_BYTES });
    assert.equal(payload.length, pkg.payload_size, `${name}: payload size`);
    assert.equal(digest(payload), pkg.payload_sha256, `${name}: payload hash`);
    const item = inventory.get(name);
    assert.deepEqual(item, { name, artifact_sha256: pkg.artifact_sha256, payload_sha256: pkg.payload_sha256 }, `${name}: release manifest`);
    const proofFile = join(directory, proofName(name));
    assert.ok(existsSync(proofFile), `${name}: missing platform signature evidence`);
    const proof = readJson(proofFile);
    assert.equal(proof.schemaVersion, 1, `${name}: unsupported signature evidence`);
    assert.equal(proof.platform, pkg.platform, `${name}: proof platform`);
    assert.equal(proof.arch, pkg.arch, `${name}: proof architecture`);
    assert.equal(proof.artifact_sha256, pkg.artifact_sha256, `${name}: proof artifact hash`);
    assert.equal(proof.payload_sha256, pkg.payload_sha256, `${name}: proof payload hash`);
    assert.ok(platformVerifiers.has(proof.verifier), `${name}: unrecognized platform verifier`);
    assert.notEqual(proof.verifier, 'fixture-unverified', `${name}: unsigned fixture cannot be published`);
    assert.ok(typeof proof.verifiedAt === 'string' && !Number.isNaN(Date.parse(proof.verifiedAt)), `${name}: missing verification time`);
    const tag = new URL(pkg.url).pathname.split('/').at(-2);
    if (!releases.has(tag)) releases.set(tag, []);
    releases.get(tag).push({ name, file, digest: pkg.artifact_sha256 }, { name: proofName(name), file: proofFile, digest: digest(readFileSync(proofFile)) });
  }
  return { bytes, releases, manifestPath };
}

function gh(args, options = {}) {
  return execFileSync('gh', args, { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024, ...options });
}
function api(path, method = 'GET', input) {
  return JSON.parse(gh(['api', `repos/${repository}/${path}`, '--method', method, ...(input ? ['--input', '-'] : [])], input ? { input: JSON.stringify(input) } : {}));
}
function release(tag) {
  try { return api(`releases/tags/${tag}`); }
  catch (error) {
    if (!String(error.stderr).includes('HTTP 404')) throw error;
    return api('releases', 'POST', { tag_name: tag, name: tag, draft: true, make_latest: 'false' });
  }
}

async function publish(directory) {
  const { bytes, releases, manifestPath } = await verifyCandidate(directory);
  // Runtime assets are immutable. Retries may only reuse byte-identical assets.
  for (const [tag, assets] of releases) {
    const published = release(tag);
    for (const asset of assets) {
      const existing = published.assets.find((item) => item.name === asset.name);
      if (existing) {
        const remote = gh(['api', `repos/${repository}/releases/assets/${existing.id}`, '-H', 'Accept: application/octet-stream'], { encoding: null });
        assert.equal(digest(remote), asset.digest, `refusing to replace a different released asset: ${asset.name}`);
      } else gh(['release', 'upload', tag, asset.file, '--repo', repository]);
    }
    const verified = release(tag);
    for (const asset of assets) {
      const remoteAsset = verified.assets.find((item) => item.name === asset.name);
      assert.ok(remoteAsset, `published asset missing: ${asset.name}`);
      const remote = gh(['api', `repos/${repository}/releases/assets/${remoteAsset.id}`, '-H', 'Accept: application/octet-stream'], { encoding: null });
      assert.equal(digest(remote), asset.digest, `published asset digest mismatch: ${asset.name}`);
    }
    api(`releases/${published.id}`, 'PATCH', { draft: false, make_latest: 'false' });
  }
  // The catalog is the public pointer and goes last, after all assets verify.
  const catalogRelease = release('app-catalog-v3');
  for (const file of ['catalog-v3.json', 'catalog-v3.sig', RELEASE_MANIFEST]) {
    gh(['release', 'upload', 'app-catalog-v3', join(directory, file), '--clobber', '--repo', repository]);
  }
  const verifiedCatalog = release('app-catalog-v3');
  const download = (name) => {
    const asset = verifiedCatalog.assets.find((item) => item.name === name);
    assert.ok(asset, `published catalog asset missing: ${name}`);
    return gh(['api', `repos/${repository}/releases/assets/${asset.id}`, '-H', 'Accept: application/octet-stream'], { encoding: null });
  };
  const remoteCatalog = download('catalog-v3.json');
  await verifyCatalogSignature({ catalogBytes: remoteCatalog, signatureB64: download('catalog-v3.sig').toString('utf8') });
  assert.equal(digest(remoteCatalog), digest(bytes), 'published catalog bytes changed');
  api(`releases/${catalogRelease.id}`, 'PATCH', { draft: false, make_latest: 'false' });
  console.log(JSON.stringify({ published: true, catalog_sha256: digest(bytes), manifest: manifestPath }));
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const directory = resolve(process.argv.slice(2).find((arg) => !arg.startsWith('-')) || 'dist/app-release');
  if (process.argv.includes('--publish')) await publish(directory);
  else {
    const { bytes, releases } = await verifyCandidate(directory);
    console.log(JSON.stringify({ verified: true, catalog_sha256: digest(bytes), releases: [...releases.keys()], packages: [...releases.values()].flat().filter((asset) => asset.name.endsWith('.nap')).length }));
  }
}
