import { readFileSync, writeFileSync } from 'node:fs';
import { createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { CATALOG_PUBLIC_KEY_B64 } from '../../extension/catalog-client.js';
import { resolve } from 'node:path';

export function signCatalog(path, keyPath) {
  const text = keyPath ? readFileSync(keyPath, 'utf8') : process.env.NATIVES_CATALOG_SIGNING_KEY;
  if (!text) throw new Error('NATIVES_CATALOG_SIGNING_KEY or an explicit signing key file is required');
  let privateKey;
  if (text.startsWith('natives-catalog-ed25519-seed v1')) {
    const seed = Buffer.from(text.trim().split('\n').slice(1).join(''), 'base64');
    if (seed.length !== 32) throw new Error('invalid Ed25519 signing seed');
    privateKey = createPrivateKey({ key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]), format: 'der', type: 'pkcs8' });
  } else privateKey = createPrivateKey(text);
  const publicKey = createPublicKey(privateKey).export({ type: 'spki', format: 'der' }).subarray(-32);
  if (privateKey.asymmetricKeyType !== 'ed25519' || publicKey.toString('base64') !== CATALOG_PUBLIC_KEY_B64) {
    throw new Error('signing key does not match the compiled catalog public key');
  }
  writeFileSync(path.replace(/\.json$/, '.sig'), sign(null, readFileSync(path), privateKey).toString('base64'));
}

if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const path = resolve(process.argv[2] || 'dist/app-release/catalog-v2.json');
  signCatalog(path, process.argv[3]);
  console.log(`signed ${path}`);
}
