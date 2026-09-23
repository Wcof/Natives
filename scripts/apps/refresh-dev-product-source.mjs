// Dev-only: rebuild the local dev product source to the CURRENT project
// baseline without re-running the pkg installer. It mirrors the subset of
// scripts/installer-package.mjs + installers/macos/build-pkg.sh that the
// Core App Store needs at runtime:
//   hosts/natives-app-runtime   (debug build, ADR-0031 unified runtime)
//   modules/<appId>/ui          (built-in module UI trees)
//   product-manifest.json/.sig  (Schema 2, Ed25519 dev trust root)
// Launchers/.app/extension payloads are NOT touched here: the dev chain
// registers Native Messaging hosts directly from target/debug (dev.mjs).
// ADR-0029 (2026-09-17 dev product-source revision): the default target is
// the user-writable `~/.natives-local/product-source/` (override with
// NATIVES_DEV_PRODUCT_SOURCE) and never needs sudo. Only `--system` writes
// the root-owned /Library/Application Support/Natives-Local/ installer
// candidate, which requires an administrator password.
import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, readdirSync, statSync, rmSync, chmodSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { treeSha256 } from '../lib/tree-hash.mjs';
import { checkProductManifest } from './check-product-manifest.mjs';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
// `--system`：拷入 root-owned 安装候选（A-Local/B-Local 安装器验收用）。
// STAGE 模式（两段式）仍保留给该路径：产物先落到本地暂存目录，再由
// 管理员权限的 `--commit <dir>` 段拷入 root 目录。
const SYSTEM_SOURCE = '/Library/Application Support/Natives-Local';
const USER_SOURCE = process.env.NATIVES_DEV_PRODUCT_SOURCE
  || join(homedir(), '.natives-local', 'product-source');
const system = process.argv.includes('--system');
const SOURCE = system ? SYSTEM_SOURCE : USER_SOURCE;
const useSudo = !process.env.NO_SUDO;
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

function sudo(args, input) {
  if (!useSudo) { console.log(`[dry] sudo ${args.join(' ')}`); return; }
  const result = spawnSync('sudo', ['-n', ...args], { input, stdio: ['pipe', 'inherit', 'inherit'] });
  if (result.status !== 0) throw new Error(`sudo ${args.join(' ')} failed (无免密 sudo 权限时请手动执行)`);
}

function rustTargetArch() {
  return execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n').find((l) => l.startsWith('host:')).split(':')[1].trim()
    .includes('aarch64') ? 'arm64' : 'x64';
}

// 1. 统一 App Runtime：与 npm run dev 同为 debug 构建（is_production_build()
//    为否 → local 命名空间 / 开发信任根，见 app_signing.rs）。
console.log('building natives-app-runtime (debug)…');
execFileSync('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'app-runtime'],
  { stdio: 'inherit', cwd: ROOT });
const appRuntimeBin = resolve(ROOT, 'target/debug/natives-app-runtime');
if (!existsSync(appRuntimeBin)) throw new Error('missing natives-app-runtime debug binary');
const appRuntimeSha256 = digest(readFileSync(appRuntimeBin));

// 2. 内置模块 UI 树：与 installer-package.mjs 同源（modules/registry.json）。
const registry = JSON.parse(readFileSync(resolve(ROOT, 'modules/registry.json'), 'utf8'));
const modules = registry.apps.map((entry) => {
  const meta = JSON.parse(readFileSync(resolve(ROOT, entry.manifest), 'utf8'));
  const uiDist = resolve(ROOT, 'modules', entry.appId, 'ui/dist');
  if (!existsSync(join(uiDist, 'index.html'))) {
    throw new Error(`module ${entry.appId} UI missing: modules/${entry.appId}/ui/dist/index.html`);
  }
  return {
    appId: meta.appId,
    // 显示名进签名清单：Core configure_product 以 modules[].name.zh_CN
    // 写入 apps.name（缺失时回退 appId），改名必须同步这里。
    name: meta.displayName,
    entryRoute: meta.entryRoute,
    moduleApiVersion: meta.moduleApiVersion,
    dataSchemaVersion: meta.dataSchemaVersion,
    capabilityVersion: meta.capabilityVersion,
    ui: { path: `modules/${meta.appId}/ui`, treeSha256: treeSha256(uiDist) },
  };
});

// 3. Schema 2 清单 + dev 信任根签名（installer-package.mjs 的 local 分支）。
// launcher/extension 哈希省略：字段缺失时 configure_product 视为旧候选保持兼容。
const manifest = {
  schemaVersion: 2,
  product: 'natives',
  version: JSON.parse(readFileSync(resolve(ROOT, 'package.json'), 'utf8')).version,
  platform: 'darwin',
  arch: rustTargetArch(),
  appRuntime: {
    protocolVersion: 2,
    path: 'hosts/natives-app-runtime',
    bytes: statSync(appRuntimeBin).size,
    sha256: appRuntimeSha256,
  },
  modules,
};
checkProductManifest(manifest);
const manifestBytes = Buffer.from(JSON.stringify(manifest, null, 2) + '\n');
const keyPath = resolve(ROOT, 'scripts/apps/keys/product-trust-dev.private.pem');
const signature = execFileSync('openssl', ['pkeyutl', '-sign', '-inkey', keyPath, '-rawin',
  '-in', '/dev/stdin', '-out', '/dev/stdout'], { input: manifestBytes, maxBuffer: 1 << 20 }).toString('base64');

// 4. 落盘：STAGE 模式先写暂存目录（仅 --system 路径需要）；commit 模式
//    从暂存目录拷入系统源；默认模式直接写用户目录产品源（零 sudo）。
function writeSource(source, useElevate, commitFrom) {
  const place = useElevate ? (args, input) => sudo(args, input) : (args) => {
    // 用户目录：等价的直接文件操作，不需要管理员权限。
    if (args[0] === 'mkdir' && args[1] === '-p') mkdirSync(args[2], { recursive: true });
    else if (args[0] === 'install') {
      const mode = parseInt(args[1].replace('-m', ''), 8); // install -m 是八进制
      copyFileSync(args[2], args[3]);
      chmodSync(args[3], mode);
    } else if (args[0] === 'rm' && args[1] === '-rf') rmSync(args[2], { recursive: true, force: true });
    else if (args[0] === 'cp' && args[1] === '-R') execFileSync('cp', ['-R', args[2], args[3]]);
    else throw new Error(`unhandled place step: ${args.join(' ')}`);
  };
  mkdirSync(source, { recursive: true });
  if (commitFrom) {
    place(['mkdir', '-p', `${source}/hosts`]);
    place(['mkdir', '-p', `${source}/modules`]);
    place(['install', '-m755', `${commitFrom}/hosts/natives-app-runtime`, `${source}/hosts/natives-app-runtime`]);
    for (const module of modules) {
      place(['rm', '-rf', `${source}/modules/${module.appId}`]);
      place(['mkdir', '-p', `${source}/modules/${module.appId}/ui`]);
      for (const entry of readdirSync(join(commitFrom, 'modules', module.appId, 'ui'), { withFileTypes: true })) {
        const src = join(commitFrom, 'modules', module.appId, 'ui', entry.name);
        if (entry.isDirectory()) place(['cp', '-R', src, `${source}/modules/${module.appId}/ui/${entry.name}`]);
        else place(['install', '-m644', src, `${source}/modules/${module.appId}/ui/${entry.name}`]);
      }
    }
    place(['install', '-m644', `${commitFrom}/product-manifest.json`, `${source}/product-manifest.json`]);
    place(['install', '-m644', `${commitFrom}/product-manifest.sig`, `${source}/product-manifest.sig`]);
  } else {
    place(['mkdir', '-p', `${source}/hosts`]);
    place(['mkdir', '-p', `${source}/modules`]);
    place(['install', '-m755', appRuntimeBin, `${source}/hosts/natives-app-runtime`]);
    for (const module of modules) {
      const uiDist = resolve(ROOT, 'modules', module.appId, 'ui/dist');
      place(['rm', '-rf', `${source}/modules/${module.appId}`]);
      place(['mkdir', '-p', `${source}/modules/${module.appId}/ui`]);
      for (const entry of readdirSync(uiDist, { withFileTypes: true })) {
        const src = join(uiDist, entry.name);
        if (entry.isDirectory()) place(['cp', '-R', src, `${source}/modules/${module.appId}/ui/${entry.name}`]);
        else place(['install', '-m644', src, `${source}/modules/${module.appId}/ui/${entry.name}`]);
      }
    }
    if (useElevate) {
      sudo(['install', '-m644', '/dev/stdin', `${source}/product-manifest.json`], manifestBytes);
      sudo(['install', '-m644', '/dev/stdin', `${source}/product-manifest.sig`], `${signature}\n`);
    } else {
      writeFileSync(join(source, 'product-manifest.json'), manifestBytes);
      writeFileSync(join(source, 'product-manifest.sig'), `${signature}\n`);
    }
  }
}

const stageDir = process.env.STAGE_DIR;
if (stageDir && system) {
  mkdirSync(join(stageDir, 'hosts'), { recursive: true });
  copyFileSync(appRuntimeBin, join(stageDir, 'hosts/natives-app-runtime'));
  for (const module of modules) {
    const uiDst = join(stageDir, 'modules', module.appId, 'ui');
    mkdirSync(uiDst, { recursive: true });
    for (const entry of readdirSync(resolve(ROOT, 'modules', module.appId, 'ui/dist'), { withFileTypes: true })) {
      const src = resolve(ROOT, 'modules', module.appId, 'ui/dist', entry.name);
      if (entry.isDirectory()) execFileSync('cp', ['-R', src, join(uiDst, entry.name)]);
      else copyFileSync(src, join(uiDst, entry.name));
    }
  }
  writeFileSync(join(stageDir, 'product-manifest.json'), manifestBytes);
  writeFileSync(join(stageDir, 'product-manifest.sig'), `${signature}\n`);
  console.log(`staged to ${stageDir}`);
  console.log(`  appRuntime sha256: ${appRuntimeSha256}`);
  console.log(`  modules: ${modules.map((m) => m.appId).join(', ')}`);
  console.log('下一步：以管理员权限执行本脚本 --commit <stageDir> --system 拷入 root 系统源。');
} else {
  const commitFrom = process.argv[2] === '--commit' ? process.argv[3] : undefined;
  writeSource(SOURCE, system, commitFrom);
  console.log(`refreshed ${SOURCE}${system ? ' (root 系统源，安装候选)' : ' (用户目录，零 sudo)'}`);
  console.log(`  appRuntime sha256: ${appRuntimeSha256}`);
  console.log(`  modules: ${modules.map((m) => m.appId).join(', ')}`);
  console.log('重新运行 npm run dev 后，扩展首次 apps:handshake 会触发 configure_product 重写 v2 activation。');

  // 同步更新 ~/.natives-local/apps/*/activation.json 中的 appRuntimeSha256，
  // 确保开发者重新运行本脚本后，无需等待扩展握手即可立刻使最新二进制通过校验。
  const appsDir = join(homedir(), '.natives-local', 'apps');
  if (existsSync(appsDir)) {
    for (const entry of readdirSync(appsDir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const actPath = join(appsDir, entry.name, 'activation.json');
      if (existsSync(actPath)) {
        try {
          const act = JSON.parse(readFileSync(actPath, 'utf8'));
          act.appRuntimeSha256 = appRuntimeSha256;
          writeFileSync(actPath, JSON.stringify(act, null, 2) + '\n');
        } catch {}
      }
    }
  }
}
