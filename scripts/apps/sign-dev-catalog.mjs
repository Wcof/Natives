#!/usr/bin/env node
// Sign the BUILD-TIME embedded dev catalog (extension/apps/catalog-v1.json)
// with the Natives-App-Catalog dev key, producing extension/apps/catalog-v1.sig.
//
// D44: the catalog URL is build-time fixed. In dev the "fixed URL" is the
// embedded pair; the release pipeline re-runs this with the production key
// and embeds the production pair. The private key is NEVER committed here —
// it is read from the sibling Natives-App-Catalog checkout (keys/catalog-key.pem),
// which is gitignored in that repo.

import { sign as edSign, createPrivateKey } from 'node:crypto';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const coreRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const catalogPath = join(coreRoot, 'extension', 'apps', 'catalog-v1.json');
const sigPath = join(coreRoot, 'extension', 'apps', 'catalog-v1.sig');
const keyPath = join(
  dirname(coreRoot),
  'Natives-App-Catalog',
  'keys',
  'catalog-key.pem',
);

const ED25519_PKCS8_HEADER = Buffer.from('302e020100300506032b657004220420', 'hex');

if (!existsSync(keyPath)) {
  console.error(`missing dev key: ${keyPath}`);
  console.error('clone Natives-App-Catalog next to this checkout and run: node scripts/sign-catalog.mjs keygen');
  process.exit(1);
}
const lines = readFileSync(keyPath, 'utf8').trim().split('\n');
if (lines[0] !== 'natives-catalog-ed25519-seed v1') throw new Error('unknown key file format');
const seed = Buffer.from(lines.slice(1).join(' '), 'base64');
if (seed.length !== 32) throw new Error('seed must be 32 bytes');
const key = createPrivateKey({
  key: Buffer.concat([ED25519_PKCS8_HEADER, seed]),
  format: 'der',
  type: 'pkcs8',
});
const data = readFileSync(catalogPath);
const sig = edSign(null, data, key);
writeFileSync(sigPath, Buffer.from(sig).toString('base64'));
console.log(JSON.stringify({ ok: true, sig: sigPath, bytes: data.byteLength }));
