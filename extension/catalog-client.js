// ADR-0025 D8/D43/D44: signed catalog client (browser side).
//
// The catalog pair (catalog-v1.json + catalog-v1.sig) is fetched from a
// BUILD-TIME fixed URL, the Ed25519 signature is verified with WebCrypto
// against a COMPILED-IN public key, and only then is the catalog parsed.
// No user-configurable URL, no user-configurable key, no network code
// outside fetch(). Verification runs BEFORE any package download.
//
// Gate coverage (check-catalog-signature.mjs mirrors these rules):
//   - official catalog          → verified
//   - catalog changed 1 byte    → rejected
//   - unknown public key        → rejected
//   - empty signature           → rejected

// Raw 32-byte Ed25519 public key (base64). Dev key from the
// Natives-App-Catalog repository; production key is swapped at build
// time by the release pipeline. Never user-configurable.
export const CATALOG_PUBLIC_KEY_B64 =
  '1QP+08RLgHdsf1Y2Oiv2K1ON5MtAFtz5sRbt0IuD1Iw=';

// Build-time fixed catalog URL (D44). Dev: GitHub Releases; production:
// own domain + CDN. Changing this is a build change, not a user setting.
export const CATALOG_URL =
  'https://raw.githubusercontent.com/Wcof/Natives-App-Catalog/main/catalog-v1.json';
export const CATALOG_SIG_URL =
  'https://raw.githubusercontent.com/Wcof/Natives-App-Catalog/main/catalog-v1.sig';

const CATALOG_MAX_BYTES = 1024 * 1024; // catalog is metadata; 1 MiB is generous

function b64ToBytes(b64) {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

function bytesToB64(bytes) {
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

function requireCrypto() {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) throw new Error('WebCrypto unavailable');
  return subtle;
}

export function newSignatureError(message) {
  const error = new Error(message);
  error.code = 'CATALOG_SIGNATURE_INVALID';
  return error;
}

export async function verifyCatalogSignature({
  catalogBytes,
  signatureB64,
  publicKeyB64 = CATALOG_PUBLIC_KEY_B64,
  subtle = requireCrypto(),
} = {}) {
  if (!catalogBytes || catalogBytes.byteLength === 0) {
    throw newSignatureError('empty catalog');
  }
  if (catalogBytes.byteLength > CATALOG_MAX_BYTES) {
    throw newSignatureError('catalog too large');
  }
  const sigB64 = String(signatureB64 || '').trim();
  if (!sigB64) throw newSignatureError('empty signature');
  const signature = b64ToBytes(sigB64);
  if (signature.byteLength !== 64) {
    throw newSignatureError('signature must be 64 bytes');
  }
  const publicKey = b64ToBytes(publicKeyB64);
  if (publicKey.byteLength !== 32) {
    throw newSignatureError('public key must be 32 bytes');
  }
  const key = await subtle.importKey(
    'raw',
    publicKey,
    { name: 'Ed25519' },
    false,
    ['verify'],
  );
  let ok = false;
  try {
    ok = await subtle.verify('Ed25519', key, signature, catalogBytes);
  } catch {
    ok = false; // WebCrypto throws on malformed input — treat as invalid
  }
  if (!ok) throw newSignatureError('catalog signature verification failed');
  return true;
}

// Fetch + verify + parse the catalog. Throws CatalogError on any failure;
// the caller (App Center) treats a bad catalog as "catalog unavailable",
// never as a reason to install from an unverified source.
export async function loadVerifiedCatalog({
  catalogUrl = CATALOG_URL,
  sigUrl = CATALOG_SIG_URL,
  publicKeyB64 = CATALOG_PUBLIC_KEY_B64,
  fetchImpl = (url) => fetch(url, { cache: 'no-store' }),
} = {}) {
  const [catalogRes, sigRes] = await Promise.all([fetchImpl(catalogUrl), fetchImpl(sigUrl)]);
  if (!catalogRes.ok) throw newSignatureError(`catalog HTTP ${catalogRes.status}`);
  if (!sigRes.ok) throw newSignatureError(`signature HTTP ${sigRes.status}`);
  const catalogBytes = new Uint8Array(await catalogRes.arrayBuffer());
  const signatureB64 = await sigRes.text();
  await verifyCatalogSignature({ catalogBytes, signatureB64, publicKeyB64 });
  let catalog;
  try {
    catalog = JSON.parse(new TextDecoder().decode(catalogBytes));
  } catch {
    throw newSignatureError('catalog is not valid JSON');
  }
  if (!catalog || !Array.isArray(catalog.apps)) {
    throw newSignatureError('catalog: missing apps[]');
  }
  return catalog;
}

// ── Package transfer (D3/D8/D10) ─────────────────────────────────────
// downloadAndStagePackage runs the browser half of the install chain:
//   fetch (streaming ≤5 MiB) → artifactSha256 → gzip decompress (≤20 MiB)
//   → payloadSha256 → base64 → returned to the caller, which sends it to
//   apps:install_package. All gates fail CLOSED; the host re-checks size
//   and payload hash independently.

export const PACKAGE_MAX_WIRE_BYTES = 5 * 1024 * 1024;
export const PACKAGE_MAX_PAYLOAD_BYTES = 20 * 1024 * 1024;

export function newPackageError(message) {
  const error = new Error(message);
  error.code = 'PACKAGE_TRANSFER_FAILED';
  return error;
}

// Streaming SHA-256 via SubtleCrypto does not exist, so we accumulate
// chunks (capped at 5 MiB — a hard budget, not a growth loop) and hash
// once. Memory stays bounded by the wire gate.
export async function downloadNapPackage({
  url,
  wireSize,
  fetchImpl = (u) => fetch(u, { cache: 'no-store' }),
  digest = (bytes) => globalThis.crypto.subtle.digest('SHA-256', bytes),
  decompress = null,
} = {}) {
  if (typeof wireSize === 'number' && wireSize > PACKAGE_MAX_WIRE_BYTES) {
    throw newPackageError(
      `wire size ${wireSize} exceeds the ${PACKAGE_MAX_WIRE_BYTES} byte (5 MiB) gate`,
    );
  }
  const response = await fetchImpl(url);
  if (!response.ok) throw newPackageError(`download HTTP ${response.status}`);
  // Stream the body so an over-budget file aborts mid-download instead
  // of allocating the whole thing.
  const chunks = [];
  let total = 0;
  if (response.body && typeof response.body.getReader === 'function') {
    const reader = response.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value) {
        total += value.byteLength;
        if (total > PACKAGE_MAX_WIRE_BYTES) {
          await reader.cancel().catch(() => {});
          throw newPackageError(
            `download exceeded the ${PACKAGE_MAX_WIRE_BYTES} byte (5 MiB) wire gate`,
          );
        }
        chunks.push(value);
      }
    }
  } else {
    const bytes = new Uint8Array(await response.arrayBuffer());
    total = bytes.byteLength;
    if (total > PACKAGE_MAX_WIRE_BYTES) {
      throw newPackageError(
        `download exceeded the ${PACKAGE_MAX_WIRE_BYTES} byte (5 MiB) wire gate`,
      );
    }
    chunks.push(bytes);
  }
  const artifact = new Uint8Array(total);
  {
    let offset = 0;
    for (const chunk of chunks) {
      artifact.set(chunk, offset);
      offset += chunk.byteLength;
    }
  }
  return { artifactBytes: artifact, wireSize: total, digest, decompress };
}

// gzip → payload with the 20 MiB decompression-bomb cap. Uses the native
// DecompressionStream (D15: no gzip library in the bundle).
export async function decompressNap(
  { artifactBytes, digest, decompress: decompressImpl },
  {
    artifactSha256,
    payloadSha256,
    payloadSize,
    decoder = null,
  } = {},
) {
  const hex = (bytes) =>
    Array.from(new Uint8Array(bytes), (b) => b.toString(16).padStart(2, '0')).join('');
  if (artifactSha256) {
    const artifactHash = hex(await digest(artifactBytes));
    if (artifactHash !== artifactSha256.toLowerCase()) {
      throw newPackageError('artifactSha256 mismatch');
    }
  }
  let payload;
  if (decoder) {
    // Test injection point (no DecompressionStream in node).
    payload = await decoder(artifactBytes);
  } else if (globalThis.DecompressionStream) {
    const decompressor = new DecompressionStream('gzip');
    const blob = new Blob([artifactBytes]);
    const decompressed = blob.stream().pipeThrough(decompressor);
    const reader = decompressed.getReader();
    const chunks = [];
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value) {
        size += value.byteLength;
        if (size > PACKAGE_MAX_PAYLOAD_BYTES) {
          await reader.cancel().catch(() => {});
          throw newPackageError(
            `payload exceeded the ${PACKAGE_MAX_PAYLOAD_BYTES} byte (20 MiB) gate`,
          );
        }
        chunks.push(value);
      }
    }
    payload = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      payload.set(chunk, offset);
      offset += chunk.byteLength;
    }
  } else {
    throw newPackageError('DecompressionStream unavailable');
  }
  if (typeof payloadSize === 'number' && payload.byteLength !== payloadSize) {
    throw newPackageError(
      `payload size ${payload.byteLength} != catalog payloadSize ${payloadSize}`,
    );
  }
  if (payloadSha256) {
    const payloadHash = hex(await digest(payload));
    if (payloadHash !== payloadSha256.toLowerCase()) {
      throw newPackageError('payloadSha256 mismatch');
    }
  }
  return { payloadBytes: payload, payloadSize: payload.byteLength, payloadSha256: hex(await digest(payload)) };
}

export function payloadToBase64(payloadBytes) {
  return bytesToB64(payloadBytes);
}

export { b64ToBytes, bytesToB64 };
