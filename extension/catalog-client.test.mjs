// Gate A5 (browser half): signed catalog verification + NAP package
// transfer with the exact budget boundaries (ADR-0025 D3/D8/D10).
//
// Runs under Node 22 (DecompressionStream + WebCrypto are available).
// No network: fetch is always mocked.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';

import {
  CATALOG_PUBLIC_KEY_B64,
  verifyCatalogSignature,
  downloadNapPackage,
  decompressNap,
  payloadToBase64,
  newPackageError,
  PACKAGE_MAX_WIRE_BYTES,
  PACKAGE_MAX_PAYLOAD_BYTES,
} from './catalog-client.js';

const sha256Hex = (bytes) =>
  Array.from(new Uint8Array(createHash('sha256').update(bytes).digest()), (b) =>
    b.toString(16).padStart(2, '0'),
  ).join('');

const digest = async (bytes) => globalThis.crypto.subtle.digest('SHA-256', bytes);

// ── fetch mock ─────────────────────────────────────────────────────────
function mockResponse(bytes, { status = 200, chunkSize = 256 * 1024 } = {}) {
  const reader = {
    index: 0,
    cancelled: false,
    async read() {
      if (this.index >= bytes.length) return { done: true, value: undefined };
      const value = bytes.slice(this.index, this.index + chunkSize);
      this.index += chunkSize;
      return { done: false, value };
    },
    async cancel() { this.cancelled = true; },
  };
  return {
    ok: status >= 200 && status < 300,
    status,
    body: { getReader: () => reader },
    async arrayBuffer() { return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength); },
    async text() { return new TextDecoder().decode(bytes); },
    async json() { return JSON.parse(new TextDecoder().decode(bytes)); },
  };
}

// ── 1. Signature gate (check-catalog-signature mirror) ─────────────────
// Official dev catalog pair embedded in the extension must verify.
{
  const catalog = readFileSync(new URL('./apps/catalog-v1.json', import.meta.url));
  const sig = readFileSync(new URL('./apps/catalog-v1.sig', import.meta.url), 'utf8').trim();
  const catalogBytes = new Uint8Array(catalog);
  await verifyCatalogSignature({ catalogBytes, signatureB64: sig });
  console.log('catalog: official dev catalog verified');

  // 1-byte tamper → FAIL
  const tampered = new Uint8Array(catalogBytes);
  tampered[10] ^= 1;
  await assert.rejects(
    () => verifyCatalogSignature({ catalogBytes: tampered, signatureB64: sig }),
    (e) => e.code === 'CATALOG_SIGNATURE_INVALID',
    '1-byte catalog tamper must be rejected',
  );

  // Unknown public key → FAIL
  const otherKey = Buffer.alloc(32, 7).toString('base64');
  await assert.rejects(
    () => verifyCatalogSignature({ catalogBytes: catalogBytes, signatureB64: sig, publicKeyB64: otherKey }),
    (e) => e.code === 'CATALOG_SIGNATURE_INVALID',
    'unknown public key must be rejected',
  );

  // Empty signature → FAIL
  await assert.rejects(
    () => verifyCatalogSignature({ catalogBytes: catalogBytes, signatureB64: '' }),
    (e) => e.code === 'CATALOG_SIGNATURE_INVALID',
    'empty signature must be rejected',
  );
  await assert.rejects(
    () => verifyCatalogSignature({ catalogBytes: catalogBytes, signatureB64: 'AAAA' }),
    (e) => e.code === 'CATALOG_SIGNATURE_INVALID',
    'short signature must be rejected',
  );
  console.log('catalog: tamper/unknown-key/empty-sig all rejected');
}

// ── 2. Wire gate exact boundaries (D3) ────────────────────────────────
// 5,242,879 PASS / 5,242,880 PASS / 5,242,881 FAIL
{
  assert.equal(PACKAGE_MAX_WIRE_BYTES, 5 * 1024 * 1024);
  const near = 5242879;
  for (const size of [near, near + 1]) {
    const bytes = new Uint8Array(size); // zero-filled: fine, gate is on size
    const fetchImpl = async () => mockResponse(bytes, { chunkSize: 1024 * 1024 });
    const result = await downloadNapPackage({ url: 'mock', wireSize: size, fetchImpl });
    assert.equal(result.wireSize, size, `wire ${size} must pass`);
  }
  const over = new Uint8Array(near + 2);
  const fetchImplOver = async () => mockResponse(over, { chunkSize: 1024 * 1024 });
  await assert.rejects(
    () => downloadNapPackage({ url: 'mock', wireSize: near + 2, fetchImpl: fetchImplOver }),
    (e) => e.message.includes('5 MiB'),
    '5,242,881 bytes must FAIL',
  );
  // catalog-declared wire_size above the gate fails before any download
  await assert.rejects(
    () => downloadNapPackage({ url: 'mock', wireSize: near + 2, fetchImpl: async () => { throw new Error('must not fetch'); } }),
    (e) => e.message.includes('5 MiB'),
  );
  console.log('wire gate: 5242879 PASS / 5242880 PASS / 5242881 FAIL');
}

// ── 3. Real .nap end-to-end (artifact hash → gzip → payload hash) ─────
{
  const nap = readFileSync(new URL('./apps/packages/demo-host-darwin-arm64.nap', import.meta.url));
  const artifactSha256 = sha256Hex(nap);
  const { gunzipSync } = await import('node:zlib');
  const payload = gunzipSync(nap);
  const payloadSha256 = sha256Hex(payload);

  const fetchImpl = async () => mockResponse(nap, { chunkSize: 64 * 1024 });
  const { artifactBytes, wireSize, digest: d } = await downloadNapPackage({
    url: 'mock.nap',
    wireSize: nap.byteLength,
    fetchImpl,
  });
  const out = await decompressNap(
    { artifactBytes, digest: d },
    { artifactSha256, payloadSha256, payloadSize: payload.byteLength },
  );
  assert.equal(out.payloadSize, payload.byteLength);
  assert.equal(out.payloadSha256, payloadSha256);
  const b64 = payloadToBase64(out.payloadBytes);
  assert.equal(b64.length, Math.ceil(payload.byteLength / 3) * 4, 'base64 length');
  assert.equal(wireSize, nap.byteLength);
  assert.ok(nap.byteLength <= PACKAGE_MAX_WIRE_BYTES, 'demo .nap under 5 MiB');
  console.log(`nap e2e: wire=${wireSize}B payload=${out.payloadSize}B base64=${b64.length}B OK`);

  // artifact hash tamper (1 byte in the .nap) → FAIL
  const badNap = new Uint8Array(nap);
  badNap[200] ^= 1;
  const badResult = await downloadNapPackage({
    url: 'mock.nap', wireSize: badNap.byteLength,
    fetchImpl: async () => mockResponse(badNap),
  });
  await assert.rejects(
    () => decompressNap({ artifactBytes: badResult.artifactBytes, digest: badResult.digest }, { artifactSha256 }),
    (e) => e.message === 'artifactSha256 mismatch',
  );
  console.log('hash gate: artifact 1-byte tamper rejected');

  // declared payload hash mismatch → FAIL
  const out2 = await downloadNapPackage({ url: 'm', wireSize: nap.byteLength, fetchImpl });
  await assert.rejects(
    () => decompressNap({ artifactBytes: out2.artifactBytes, digest: out2.digest }, {
      artifactSha256,
      payloadSha256: '1'.repeat(64),
      payloadSize: payload.byteLength,
    }),
    (e) => e.message === 'payloadSha256 mismatch',
  );
  // declared payload size mismatch → FAIL
  await assert.rejects(
    () => decompressNap({ artifactBytes: out2.artifactBytes, digest: out2.digest }, {
      artifactSha256, payloadSha256, payloadSize: payload.byteLength + 1,
    }),
    (e) => e.message.includes('payload size'),
  );
  console.log('hash gate: payload hash/size mismatch rejected');
}

// ── 4. Decompression bomb (D7: wire 1 MiB → payload >20 MiB) ──────────
{
  assert.equal(PACKAGE_MAX_PAYLOAD_BYTES, 20 * 1024 * 1024);
  // 21 MiB of zeros gzips to a few KB (well under 5 MiB wire) but
  // decompresses past the 20 MiB payload cap.
  const bombPayload = new Uint8Array(21 * 1024 * 1024);
  const bombNap = new Uint8Array(gzipSync(bombPayload, { level: 9 }));
  assert.ok(bombNap.byteLength < 1024 * 1024, `bomb wire ${bombNap.byteLength} < 1 MiB`);
  const { artifactBytes, digest: d } = await downloadNapPackage({
    url: 'bomb.nap', wireSize: bombNap.byteLength,
    fetchImpl: async () => mockResponse(bombNap, { chunkSize: 32 * 1024 }),
  });
  await assert.rejects(
    () => decompressNap({ artifactBytes, digest: d }, {}),
    (e) => e.message.includes('20 MiB'),
    'decompression bomb must be rejected at the payload cap',
  );
  console.log(`bomb: wire=${bombNap.byteLength}B → payload 21MiB rejected`);
}

// ── 5. Host-side re-check constants parity ────────────────────────────
// The Rust host re-implements the same gates (app_store/types.rs).
// Keep the two definitions from drifting.
{
  assert.equal(PACKAGE_MAX_WIRE_BYTES, 5242880);
  assert.equal(PACKAGE_MAX_PAYLOAD_BYTES, 20971520);
  console.log('constants: wire 5MiB / payload 20MiB parity');
}

console.log('catalog-client: Gate A5 browser-side checks passed');
