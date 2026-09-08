import { readFileSync } from 'node:fs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';
await verifyCatalogSignature({
  catalogBytes: readFileSync(new URL('../../extension/apps/catalog-v1.json', import.meta.url)),
  signatureB64: readFileSync(new URL('../../extension/apps/catalog-v1.sig', import.meta.url), 'utf8'),
});
console.log('catalog Ed25519 signature passed');
