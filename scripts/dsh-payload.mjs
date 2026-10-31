#!/usr/bin/env node
// Build the sealed payload directory for the four DSH plugins (ADR-0048 D2/D6).
//
// The payload is a complete signed product source snapshot: it is the only way
// a machine without Natives can resolve a Core (resolution step 4). Layout,
// mirroring what the Core resolves through NATIVES_DEV_PRODUCT_SOURCE:
//   product-manifest.json / product-manifest.sig   (signed Schema 2 manifest)
//   hosts/native-file-host                          (Core)
//   hosts/natives-app-runtime                       (ADR-0031 unified runtime)
//   model-host                                      (AI domain host binary)
//   ChromeExtension/                                (Web static root)
//   modules/<appId>/ui/                             (built-in module UI trees)
//   payload-manifest.json                           (generated: version + per-file sha256)
//
// 交付纪律：发布用 payload 必须来自生产签名产品源。开发信任根签出的产品源
// 只能用于本机联调，不得对外分发冒充生产 payload（release 恒定拒绝
// APP_DEV_TRUST_ROOT_IN_PRODUCTION）。本脚本不做签名，只密封与校验。
//
// Usage: node scripts/dsh-payload.mjs --source <complete-product-source> [--out <dir>]
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync, chmodSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const PLUGINS = ['space', 'files', 'tokenusage', 'fund'];
// Default: write the payload into each plugin package directory so `npm pack`
// includes it via the package.json `files` entry. `--out` stages elsewhere.
const OUT_DEFAULT = join(ROOT, 'integrations', 'dsh');
const REQUIRED = ['product-manifest.json', 'product-manifest.sig', 'hosts', 'ChromeExtension', 'modules'];
const REQUIRED_HOSTS = ['native-file-host', 'natives-app-runtime'];

function parseArgs(argv) {
  const value = (name) => {
    const index = argv.indexOf(name);
    return index >= 0 ? argv[index + 1] : undefined;
  };
  const source = value('--source');
  if (!source) {
    console.error('usage: node scripts/dsh-payload.mjs --source <complete-product-source> [--out <dir>]');
    process.exit(2);
  }
  return { source: resolve(source), out: resolve(value('--out') ?? OUT_DEFAULT) };
}

function assertCompleteProductSource(source) {
  const missing = REQUIRED.filter((entry) => !existsSync(join(source, entry)));
  if (missing.length) {
    console.error(`incomplete product source ${source}: missing ${missing.join(', ')}`);
    console.error('assemble a complete signed product source first (hosts/, model-host, ChromeExtension/, modules/).');
    process.exit(1);
  }
  const missingHosts = REQUIRED_HOSTS.filter((entry) => !existsSync(join(source, 'hosts', entry)));
  if (missingHosts.length) {
    console.error(`incomplete product source ${source}: hosts/ missing ${missingHosts.join(', ')}`);
    process.exit(1);
  }
  if (!existsSync(join(source, 'model-host'))) {
    console.error(`incomplete product source ${source}: model-host binary missing at the product root`);
    process.exit(1);
  }
}

function sha256File(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function collectFiles(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...collectFiles(full));
    else if (entry.isFile()) files.push(full);
  }
  return files.sort();
}

const { source, out } = parseArgs(process.argv);
assertCompleteProductSource(source);

const productManifest = JSON.parse(readFileSync(join(source, 'product-manifest.json'), 'utf8'));
const version = productManifest?.version;
if (typeof version !== 'string' || version.length === 0) {
  console.error('product-manifest.json has no version; refusing to seal');
  process.exit(1);
}

for (const plugin of PLUGINS) {
  const payload = join(out, plugin, 'payload');
  rmSync(payload, { recursive: true, force: true });
  mkdirSync(payload, { recursive: true });

  for (const entry of [...REQUIRED, 'model-host']) {
    const from = join(source, entry);
    cpSync(from, join(payload, entry), { recursive: true });
  }
  for (const binary of [join(payload, 'hosts', 'native-file-host'), join(payload, 'hosts', 'natives-app-runtime'), join(payload, 'model-host')]) {
    chmodSync(binary, 0o755);
  }

  const files = {};
  let bytes = 0;
  for (const file of collectFiles(payload)) {
    const relativePath = relative(payload, file).split('\\').join('/');
    if (relativePath === 'payload-manifest.json') continue;
    files[relativePath] = sha256File(file);
    bytes += statSync(file).size;
  }
  writeFileSync(
    join(payload, 'payload-manifest.json'),
    JSON.stringify({ version, platform: productManifest?.platform ?? null, arch: productManifest?.arch ?? null, files }, null, 2) + '\n'
  );
  console.log(`✓ ${plugin}: payload sealed (${Object.keys(files).length} files, ${(bytes / 1_048_576).toFixed(1)} MiB, product ${version})`);
}

console.log(`\npayloads written under ${out}`);
console.log('分发前核对：product-manifest.sig 必须来自生产信任根；开发签名 payload 只限本机联调，不得对外分发。');
console.log('下一步：在 integrations/dsh/<plugin>/ 下 npm pack，或先 payload 后 pack 一并交付。');
