import { readFileSync } from 'node:fs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';

const v2Json = new URL('../../extension/apps/catalog-v2.json', import.meta.url);
const v2Sig = new URL('../../extension/apps/catalog-v2.sig', import.meta.url);
await verifyCatalogSignature({
  catalogBytes: readFileSync(v2Json),
  signatureB64: readFileSync(v2Sig, 'utf8'),
});
console.log('catalog Ed25519 signature passed');
