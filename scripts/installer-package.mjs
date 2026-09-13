// Natives 安装包输入组装器（实施方案 §5 P1）：只负责准备唯一安装引擎
// installers/macos/build-pkg.sh 的全部输入——
//   Host 二进制（local 模式用 debug 构建：本地开发信任根 + Natives-Local 源，
//               production 用 release 构建 + 正式签名身份）
//   model-host（Go 构建）
//   解压扩展目录（scripts/extension-package.mjs 的 dist/extension）
//   固定模块文件（相邻 Natives-App-Fund 的 .nap 单载荷解压为可执行）
//   已签名产品组合清单 product-manifest.json/.sig（Ed25519，dev/生产密钥）
// pkgbuild/productbuild 只发生在 build-pkg.sh；这里不再有第二布局，
// 不组装 Natives.app，不写 /Applications，不产生 seeds。
import { existsSync, mkdirSync, rmSync, writeFileSync, readFileSync, readdirSync, createReadStream, createWriteStream, chmodSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync, spawnSync } from 'node:child_process';
import { pipeline } from 'node:stream/promises';
import { createGunzip } from 'node:zlib';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const STAGING = resolve(ROOT, 'dist/installer-input');
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

function usage() {
  console.error('usage: node scripts/installer-package.mjs --mode local|production [--version V] [--output PATH] [--fund-nap PATH] [--pkg-sign ID] [--product-sign ID]');
  process.exit(2);
}

function rustTargetArch() {
  const host = execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n').find((l) => l.startsWith('host:')).split(':')[1].trim();
  return host.includes('aarch64') ? 'arm64' : 'x64';
}

function gunzipTo(napPath, outPath) {
  mkdirSync(dirname(outPath), { recursive: true });
  return pipeline(createReadStream(napPath), createGunzip(), createWriteStream(outPath));
}

// 自检：安装输入树内不得出现开发环境残留目录（node_modules/target/.git）。
function auditTree(dir) {
  const forbidden = /(^|\/)(node_modules|target|\.git)(\/|$)/i;
  const violations = [];
  (function visit(d) {
    for (const entry of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, entry.name);
      if (forbidden.test(p)) violations.push(p);
      else if (entry.isDirectory()) visit(p);
    }
  })(dir);
  if (violations.length) throw new Error('forbidden paths in installer input tree:\n' + violations.join('\n'));
  return true;
}

function buildHostBinary(mode) {
  // local 候选必须用 debug 构建的 Host：is_production_build() 为否，
  // 开发信任根与 /Library/Application Support/Natives-Local/ 源才生效
  //（contract §4.2 本地隔离身份）。production 用 release 构建。
  const profile = mode === 'production' ? 'release' : 'debug';
  execFileSync('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build',
    '-p', 'native-file-host', ...(profile === 'release' ? ['--release'] : [])], { stdio: 'inherit', cwd: ROOT });
  const bin = resolve(ROOT, `target/${profile}/native-file-host`);
  if (!existsSync(bin)) throw new Error(`missing ${bin}`);
  return bin;
}

function buildModelHost() {
  execFileSync('go', ['build', '-o', join(STAGING, 'model-host'), '.'], { stdio: 'inherit', cwd: join(ROOT, 'model-host') });
  const bin = join(STAGING, 'model-host');
  chmodSync(bin, 0o755);
  return bin;
}

function signProductManifest(manifestBytes, keyPath) {
  const input = join(STAGING, '.manifest-to-sign');
  const output = join(STAGING, '.manifest-signature');
  writeFileSync(input, manifestBytes);
  execFileSync('openssl', ['pkeyutl', '-sign', '-inkey', keyPath, '-rawin', '-in', input, '-out', output], { stdio: 'inherit' });
  const signature = readFileSync(output).toString('base64');
  rmSync(input, { force: true });
  rmSync(output, { force: true });
  return signature;
}

export async function buildInstaller({ mode = 'local', version, output, fundNap } = {}) {
  if (mode !== 'local' && mode !== 'production') usage();
  if (!version) {
    version = JSON.parse(readFileSync(join(ROOT, 'extension/manifest.json'), 'utf8')).version;
  }
  // 解压扩展目录：强制由 buildExtension 现场重建（manifest 注入稳定 key，
  // Chrome 按其派生固定 Extension ID，与加载路径无关），并核验 key 在位。
  const { buildExtension, STABLE_EXTENSION_ID } = await import('./extension-package.mjs');
  buildExtension(join(ROOT, 'dist/extension'));
  const packagedManifest = JSON.parse(readFileSync(join(ROOT, 'dist/extension/manifest.json'), 'utf8'));
  if (!packagedManifest.key) throw new Error('packaged extension manifest missing stable key');
  const extensionId = STABLE_EXTENSION_ID;
  rmSync(STAGING, { recursive: true, force: true });
  mkdirSync(STAGING, { recursive: true });

  const host = buildHostBinary(mode);
  const modelHost = buildModelHost();

  // 固定模块：fund .nap 是 gzip 单载荷，解压即 Mach-O 可执行程序；
  // 解压后与 .meta.json 声明的 payload_sha256 核对（方案 P1：输入必须
  // 与来源版本/摘要绑定）。
  const napPath = fundNap || findFundNap();
  const meta = JSON.parse(readFileSync(`${napPath}.meta.json`, 'utf8'));
  const appDir = join(STAGING, 'modules/fund', meta.version);
  mkdirSync(appDir, { recursive: true });
  const exe = join(appDir, 'app');
  await pipeline(createReadStream(napPath), createGunzip(), createWriteStream(exe));
  chmodSync(exe, 0o755);
  const payloadSha256 = digest(readFileSync(exe));
  if (meta.payload_sha256 && meta.payload_sha256 !== payloadSha256) {
    throw new Error(`fund payload hash mismatch: ${meta.payload_sha256} != ${payloadSha256}`);
  }

  // 产品组合清单：与 host 端 app_store/product.rs 的读取格式一致，
  // Ed25519 分离签名（local 用开发信任根私钥，生产由 --manifest-key 提供）。
  const appJson = JSON.parse(readFileSync(resolve(ROOT, '../Natives-App-Fund/app.json'), 'utf8'));
  const arch = rustTargetArch();
  const manifest = {
    schemaVersion: 1,
    product: 'natives',
    version,
    platform: 'darwin',
    arch,
    modules: [{
      appId: appJson.appId,
      version: appJson.version,
      entryRoute: 'app.html?app=fund',
      name: appJson.name,
      artifactPath: `modules/fund/${appJson.version}/app`,
      payloadSha256,
    }],
  };
  const manifestBytes = Buffer.from(JSON.stringify(manifest, null, 2) + '\n');
  const manifestPath = join(STAGING, 'product-manifest.json');
  writeFileSync(manifestPath, manifestBytes);
  const keyPath = resolve(ROOT, 'scripts/apps/keys/catalog-trust-dev.private.pem');
  if (!existsSync(keyPath)) throw new Error('missing development product signing key');
  const signature = signProductManifest(manifestBytes, keyPath);
  writeFileSync(`${manifestPath}.sig`, signature + '\n');

  auditTree(STAGING);

  output = output || join(ROOT, 'dist/installer', `Natives-${version}-macOS-${arch}-${mode}.pkg`);
  const engine = join(ROOT, 'installers/macos/build-pkg.sh');
  const args = ['installers/macos/build-pkg.sh',
    '--mode', mode, '--host', host, '--model-host', modelHost,
    '--extension-id', extensionId, '--extension-dir', join(ROOT, 'dist/extension'),
    '--modules-dir', join(STAGING, 'modules'),
    '--product-manifest', manifestPath,
    '--version', version, '--output', output];
  execFileSync('sh', args, { stdio: 'inherit', cwd: ROOT });
  const pkgBytes = readFileSync(output);
  const sums = [
    `${digest(pkgBytes)}  ${basename(output)}`,
    `${payloadSha256}  modules/fund/${meta.version}/app`,
  ].join('\n') + '\n';
  writeFileSync(join(dirname(output), 'SHA256SUMS'), sums);
  return { pkg: output, sha256: digest(pkgBytes), size: pkgBytes.length, extensionId, manifest };
}

function findFundNap() {
  const arch = rustTargetArch();
  const fundDist = resolve(ROOT, '../Natives-App-Fund/dist');
  const candidates = arch === 'arm64'
    ? ['fund-0.1.0-aarch64-apple-darwin.nap']
    : ['fund-0.1.0-x86_64-apple-darwin.nap'];
  for (const name of candidates) {
    if (existsSync(join(fundDist, name))) return join(fundDist, name);
  }
  throw new Error(`missing fund .nap for ${arch} in ${fundDist} (build the fund repo first)`);
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const args = process.argv.slice(2);
  const options = {};
  for (let i = 0; i < args.length; i += 1) {
    if (args[i] === '--mode') options.mode = args[++i];
    else if (args[i] === '--version') options.version = args[++i];
    else if (args[i] === '--output') options.output = args[++i];
    else if (args[i] === '--fund-nap') options.fundNap = args[++i];
  }
  buildInstaller(options).then((result) => {
    console.log(JSON.stringify({ pkg: result.pkg, sha256: result.sha256, size: result.size, extensionId: result.extensionId }));
  }).catch((error) => {
    console.error(error.message || error);
    process.exit(1);
  });
}
