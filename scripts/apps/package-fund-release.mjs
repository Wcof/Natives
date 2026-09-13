// Fund 正式 Release 产物（用户方案 §Fund）：
//   ../Natives-App-Fund/dist/fund-{version}-{triple}.nap（真实构建链产物）
//     → dist/fund-release/fund-{version}.nap
//     → SHA256SUMS
//     → catalog-v3.json（apps=[fund]，packages 指向真实 Release Artifact URL）
//     → Ed25519 签名（沿用既有 catalog-signing 规范与编译进扩展的信任根）
// 禁止：本地 fixture、测试包、自造哈希——全部字段取自真实 nap 与 meta。
import { existsSync, readFileSync, writeFileSync, copyFileSync, mkdirSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const FUND_DIR = resolve(ROOT, '../Natives-App-Fund');
const OUT = resolve(ROOT, 'dist/fund-release');

const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

function hostTriple() {
  return execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n').find((l) => l.startsWith('host:')).split(':')[1].trim();
}

function main() {
  const fundCargo = readFileSync(join(FUND_DIR, 'Cargo.toml'), 'utf8');
  const version = fundCargo.match(/version\s*=\s*"([^"]+)"/)?.[1];
  if (!version) throw new Error('cannot read Fund version from Cargo.toml');
  const triple = hostTriple();
  const napName = `fund-${version}-${triple}.nap`;
  const napPath = join(FUND_DIR, 'dist', napName);
  if (!existsSync(napPath)) throw new Error(`missing Fund artifact: ${napPath} (run ../Natives-App-Fund/scripts/package.sh first)`);
  const metaPath = `${napPath}.meta.json`;
  if (!existsSync(metaPath)) throw new Error(`missing Fund artifact metadata: ${metaPath}`);
  const meta = JSON.parse(readFileSync(metaPath, 'utf8'));

  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(OUT, { recursive: true });
  const releaseNap = join(OUT, `fund-${version}.nap`);
  copyFileSync(napPath, releaseNap);
  const napBytes = readFileSync(releaseNap);
  const artifactSha256 = digest(napBytes);
  writeFileSync(join(OUT, 'SHA256SUMS'), `${artifactSha256}  fund-${version}.nap\n`);

  // Catalog v3：与 extension/apps/catalog-v3.json 同 schema；packages 指向
  // 该仓库既有 Release 规范的 artifact URL（apps-{app_id}-v{version} tag）。
  const extensionVersion = JSON.parse(readFileSync(join(ROOT, 'extension/manifest.json'), 'utf8')).version;
  const catalog = {
    catalogVersion: 3,
    publishedAt: new Date().toISOString(),
    apps: [{
      app_id: 'fund', kind: 'managed_local', name: 'Fund', version,
      appProtocolVersion: 1,
      minExtensionVersion: extensionVersion,
      minHostVersion: '0.1.0',
      description: { zh_CN: '基金：个人资产记账', en: 'Fund: personal asset ledger' },
      icon: 'grid',
      permissions: ['app.lifecycle'],
      runtime_spec: {},
      surface: { icon: 'grid', route: 'app.html?app=fund' },
      manifest: { schemaVersion: 1 },
      packages: [{
        package_id: 'app-exec', kind: 'managed_local', version,
        platform: 'darwin', arch: triple.includes('aarch64') ? 'arm64' : 'x64', required: true,
        url: `https://github.com/Wcof/Natives/releases/download/apps-fund-v${version}/fund-${version}.nap`,
        wire_size: napBytes.length,
        payload_size: meta.payload_size,
        payload_sha256: meta.payload_sha256,
        sha256: artifactSha256,
      }],
    }],
  };
  const catalogPath = join(OUT, 'catalog-v3.json');
  writeFileSync(catalogPath, JSON.stringify(catalog, null, 2) + '\n');

  // 签名：沿用既有 catalog-signing（密钥必须匹配扩展内置信任根）；
  // 无密钥环境产出未签名 catalog 并显式标注，发布 gate 会拒绝未签名目录。
  const keyArg = process.argv.find((a) => a.startsWith('--sign='));
  let signed = false;
  if (keyArg) {
    const keyPath = keyArg.split('=')[1] || join(ROOT, 'scripts/apps/keys/catalog-trust-dev.private.pem');
    execFileSync('node', [join(ROOT, 'scripts/apps/catalog-signing.mjs'), catalogPath, keyPath], { stdio: 'inherit' });
    signed = existsSync(catalogPath.replace(/\.json$/, '.sig'));
  }
  console.log(JSON.stringify({
    nap: `fund-${version}.nap`, sha256: artifactSha256, wireSize: napBytes.length,
    payloadSha256: meta.payload_sha256, catalog: 'catalog-v3.json', signed,
  }));
}

main();
