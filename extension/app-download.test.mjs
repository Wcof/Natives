import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { CATALOG_SOURCES, artifactSources, fetchBytes, fetchFromSources } from './app-download.js';
import { loadVerifiedCatalog } from './catalog-client.js';

const url = 'https://github.com/Wcof/Natives/releases/download/apps-demo-v1.1.0/demo.nap';
const sources = artifactSources(url);
assert.equal(sources.length, 2);
for (const bad of ['https://example.com/host.nap', 'http://github.com/a', `${url}?token=x`, 'file:///etc/passwd']) {
  assert.throws(() => artifactSources(bad));
}
const attempted = [];
assert.deepEqual(await fetchFromSources(sources, { limit: 16, fetchImpl: async (source) => {
  attempted.push(source);
  if (source === sources[0]) throw new TypeError('offline');
  return new Response(new Uint8Array([1, 2, 3]));
} }), new Uint8Array([1, 2, 3]));
assert.deepEqual(attempted, sources);

let calls = 0;
await assert.rejects(fetchFromSources(sources, { limit: 1, fetchImpl: async () => {
  calls++; return new Response(new Uint8Array([1, 2]));
} }), { code: 'APP_SIZE_LIMIT' });
assert.equal(calls, 1, 'size failures must not switch source');

const abort = new AbortController();
abort.abort();
await assert.rejects(fetchFromSources(sources, { limit: 16, signal: abort.signal }), { code: 'APP_CANCELLED' });
await assert.rejects(fetchBytes(url, { limit: 16, timeoutMs: 10,
  fetchImpl: async () => new Response(new ReadableStream({ start() {} })),
}), { code: 'APP_NETWORK' }, 'a stalled response body must time out');

const catalog = readFileSync(new URL('./apps/catalog-v2.json', import.meta.url));
const signature = readFileSync(new URL('./apps/catalog-v2.sig', import.meta.url));
const urls = [];
const loaded = await loadVerifiedCatalog({ fetchImpl: async (source) => {
  urls.push(source);
  if (source.startsWith(CATALOG_SOURCES[0])) throw new TypeError('offline');
  return new Response(source.endsWith('.sig') ? signature : catalog);
} });
assert.equal(loaded.catalogVersion, 2);
assert.ok(loaded.source.startsWith(CATALOG_SOURCES[1]));
assert.deepEqual(urls.slice(1), ['catalog-v2.json', 'catalog-v2.sig'].map((name) => CATALOG_SOURCES[1] + name));
let reads = 0;
await assert.rejects(loadVerifiedCatalog({ fetchImpl: async (source) => {
  reads++;
  return new Response(source.endsWith('.sig') ? Buffer.alloc(88, 65) : catalog);
} }), { code: 'CATALOG_SIGNATURE_INVALID' });
assert.equal(reads, 2, 'signature failures must not retry another source');
console.log('app downloads: allowed sources, paired signatures, fallback, byte limits, timeout and cancellation passed');
