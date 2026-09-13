import { readFileSync } from 'node:fs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';

const v3Json = new URL('../../extension/apps/catalog-v3.json', import.meta.url);
const v3Sig = new URL('../../extension/apps/catalog-v3.sig', import.meta.url);
await verifyCatalogSignature({
  catalogBytes: readFileSync(v3Json),
  signatureB64: readFileSync(v3Sig, 'utf8'),
});
console.log('catalog Ed25519 signature passed');
