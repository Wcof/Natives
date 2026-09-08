import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
import { checkCatalog } from './check-app-manifest.mjs';
import { checkPackageBudget } from './check-package-budget.mjs';
import { verifyCatalogSignature, PACKAGE_MAX_PAYLOAD_BYTES } from '../../extension/catalog-client.js';

const directory = resolve('dist/app-release');
const bytes = readFileSync(join(directory, 'catalog-v1.json'));
await verifyCatalogSignature({ catalogBytes: bytes, signatureB64: readFileSync(join(directory, 'catalog-v1.sig'), 'utf8') });
const catalog = JSON.parse(bytes);
checkCatalog(catalog);
checkPackageBudget(catalog);
const repository = 'Wcof/Natives';
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const releases = new Map();
for (const app of catalog.apps) {
  for (const pkg of app.packages) {
    const url = new URL(pkg.url);
    const parts = url.pathname.split('/');
    const tag = parts.at(-2), name = parts.at(-1);
    assert.ok(/^[a-zA-Z0-9._-]+$/.test(tag) && /^[a-zA-Z0-9._-]+$/.test(name));
    const file = join(directory, name), nap = readFileSync(file);
    assert.equal(nap.length, pkg.wire_size, name + ' wire size');
    assert.equal(digest(nap), pkg.artifact_sha256, name + ' artifact hash');
    const payload = gunzipSync(nap, { maxOutputLength: PACKAGE_MAX_PAYLOAD_BYTES });
    assert.equal(payload.length, pkg.payload_size, name + ' payload size');
    assert.equal(digest(payload), pkg.payload_sha256, name + ' payload hash');
    if (!releases.has(tag)) releases.set(tag, []);
    releases.get(tag).push({ name, file, digest: pkg.artifact_sha256 });
  }
}
console.log(JSON.stringify({ verified: true, repository, releases: [...releases.keys()], packages: [...releases.values()].flat().length }));
if (process.argv.includes('--publish')) {
  function gh(args, options = {}) {
    return execFileSync('gh', args, { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024, ...options });
  }
  function api(path, method = 'GET', input) {
    return JSON.parse(gh(['api', `repos/${repository}/${path}`, '--method', method, ...(input ? ['--input', '-'] : [])],
      input ? { input: JSON.stringify(input) } : {}));
  }
  function release(tag) {
    try { return api(`releases/tags/${tag}`); }
    catch (error) {
      if (!String(error.stderr).includes('HTTP 404')) throw error;
      return api('releases', 'POST', { tag_name: tag, name: tag, draft: true, make_latest: 'false',
        target_commitish: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() });
    }
  }
  // Runtime artifacts are immutable. A retry may only reuse byte-identical assets.
  for (const [tag, assets] of releases) {
    const published = release(tag);
    for (const asset of assets) {
      const existing = published.assets.find((item) => item.name === asset.name);
      if (existing) {
        const remote = gh(['api', `repos/${repository}/releases/assets/${existing.id}`, '-H', 'Accept: application/octet-stream'], { encoding: null });
        assert.equal(digest(remote), asset.digest, 'refusing to replace a different released artifact');
      } else gh(['release', 'upload', tag, asset.file, '--repo', repository]);
    }
    api(`releases/${published.id}`, 'PATCH', { draft: false, make_latest: 'false' });
  }
  // Publish the catalog only after every referenced runtime release exists.
  const catalogRelease = release('app-catalog-v1');
  gh(['release', 'upload', 'app-catalog-v1', join(directory, 'catalog-v1.json'), join(directory, 'catalog-v1.sig'), '--clobber', '--repo', repository]);
  api(`releases/${catalogRelease.id}`, 'PATCH', { draft: false, make_latest: 'false' });
  console.log('App runtime artifacts and signed catalog published');
}
