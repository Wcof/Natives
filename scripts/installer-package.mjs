// Natives 安装包输入组装器（实施方案 §5 P1）：只负责准备唯一安装引擎
// installers/macos/build-pkg.sh 的全部输入——
//   Host 二进制（local 模式用 debug 构建：本地开发信任根 + Natives-Local 源，
//               production 用 release 构建 + 正式签名身份）
//   app-runtime（Rust 构建，内置各官方模块）
//   model-host（Go 构建）
//   解压扩展目录（scripts/extension-package.mjs 的 dist/extension）
//   官方内置模块 UI 静态资产（modules/fund/ui/dist/）
//   已签名产品组合清单 product-manifest.json/.sig（Schema 2，Ed25519，dev/生产密钥）
// pkgbuild/productbuild 只发生在 build-pkg.sh；这里不再有第二布局，
// 不暴露模块级安装入口；主 Natives.app 由 build-pkg.sh 组装并安装。
import { existsSync, mkdirSync, rmSync, writeFileSync, copyFileSync, readFileSync, readdirSync, chmodSync, statSync } from 'node:fs';
import { treeSha256 } from './lib/tree-hash.mjs';
import { createHash } from 'node:crypto';
import { join, resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const STAGING = resolve(ROOT, 'dist/installer-input');
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

function usage() {
  console.error('usage: node scripts/installer-package.mjs --mode local|production [--version V] [--output PATH] [--pkg-sign ID] [--product-sign ID]');
  process.exit(2);
}

function rustTargetArch() {
  const host = execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n').find((l) => l.startsWith('host:')).split(':')[1].trim();
  return host.includes('aarch64') ? 'arm64' : 'x64';
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

function buildAppRuntimeBinary(mode) {
  const profile = mode === 'production' ? 'release' : 'debug';
  execFileSync('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build',
    '-p', 'app-runtime', ...(profile === 'release' ? ['--release'] : [])], { stdio: 'inherit', cwd: ROOT });
  const bin = resolve(ROOT, `target/${profile}/natives-app-runtime`);
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

export async function buildInstaller({ mode = 'local', version, output, manifestKey, pkgSign, productSign } = {}) {
  if (mode !== 'local' && mode !== 'production') usage();
  if (!version) {
    version = JSON.parse(readFileSync(join(ROOT, 'extension/manifest.json'), 'utf8')).version;
  }
  // 解压扩展目录：强制由 buildExtension 现场重建（manifest 注入稳定 key，
  // Chrome 按其派生固定 Extension ID，与加载路径无关），并核验 key 在位。
  // 模式隔离（plan §5 P1）：local 使用独立扩展 key/ID 与 local Host 名。
  const { buildExtension, STABLE_EXTENSION_ID, LOCAL_EXTENSION_ID } = await import('./extension-package.mjs');
  buildExtension(join(ROOT, 'dist/extension'), ROOT, mode);
  const packagedManifest = JSON.parse(readFileSync(join(ROOT, 'dist/extension/manifest.json'), 'utf8'));
  if (!packagedManifest.key) throw new Error('packaged extension manifest missing stable key');
  const extensionId = mode === 'local' ? LOCAL_EXTENSION_ID : STABLE_EXTENSION_ID;
  rmSync(STAGING, { recursive: true, force: true });
  mkdirSync(STAGING, { recursive: true });

  const host = buildHostBinary(mode);
  const modelHost = buildModelHost();

  // 可见主入口包装（§1.1）：编译 ObjC 包装（AppKit 原生状态窗：重新检测/
  // 打开扩展管理页/显示扩展文件夹/复制目录路径；Finder 双击不弹 Terminal），
  // SOURCE_ROOT 固化系统源路径，双击经它调用 native-file-host 引导模式。
  const sourceName = mode === 'production' ? 'Natives' : 'Natives-Local';
  const setupScheme = mode === 'production' ? 'natives-setup' : 'natives-setup-local';
  const appExec = join(STAGING, 'Natives');
  execFileSync('cc', ['-O2', '-framework', 'AppKit', '-framework', 'WebKit', '-lsqlite3',
    '-DSOURCE_ROOT="' + `/Library/Application Support/${sourceName}` + '"',
    '-DEXTENSION_ID="' + extensionId + '"',
    '-DSETUP_SCHEME="' + setupScheme + '"',
    '-o', appExec,
    join(ROOT, 'installers/macos/resources/launcher-main.m'),
    join(ROOT, 'installers/macos/resources/NativesStatusBar.m')], { stdio: 'inherit' });
  const appIcon = join(ROOT, 'installers/macos/resources/natives.icns');

  // 随包离线引导页（§1.3）：单一内容源模板 → index.html；conclusion
  // 摘要与 HTML 从同一组变量生成，构建后断言一致。
  const extensionDir = `/Library/Application Support/${sourceName}/ChromeExtension`;
  const fill = (text) => text
    .replaceAll('__VERSION__', version)
    .replaceAll('__EXTENSION_ID__', extensionId)
    .replaceAll('__EXTENSION_DIR__', extensionDir)
    .replaceAll('__SETUP_SCHEME__', setupScheme)
    .replaceAll('__SOURCE_ROOT__', `/Library/Application Support/${sourceName}`);
  const template = readFileSync(join(ROOT, 'installers/macos/resources/onboarding-template.html'), 'utf8');
  const onboardingDir = join(STAGING, 'onboarding');
  mkdirSync(onboardingDir, { recursive: true });
  writeFileSync(join(onboardingDir, 'index.html'), fill(template));
  const conclusionDir = join(STAGING, 'conclusion');
  mkdirSync(conclusionDir, { recursive: true });
  const conclusionZh = `Natives ${version} 安装完成。请在"应用程序"中双击 Natives 完成扩展加载；扩展目录：${extensionDir}`;
  const conclusionEn = `Natives ${version} installed. Open Natives from Applications to finish loading the extension. Extension folder: ${extensionDir}`;
  writeFileSync(join(conclusionDir, 'conclusion-zh.txt'), conclusionZh + '\n');
  writeFileSync(join(conclusionDir, 'conclusion-en.txt'), conclusionEn + '\n');
  // 一致性检查（§1.3）：三处产物必须包含同一版本与目录文本。
  const finalHtml = readFileSync(join(onboardingDir, 'index.html'), 'utf8');
  for (const marker of [version, extensionDir, extensionId]) {
    if (!finalHtml.includes(marker) || !conclusionZh.includes(marker) === (marker === extensionId ? false : false)) {
      // 目录与版本必须同源；扩展 ID 只要求出现在 HTML。
    }
    if (!finalHtml.includes(marker)) throw new Error(`onboarding missing ${marker}`);
  }
  if (!conclusionZh.includes(version) || !conclusionZh.includes(extensionDir)) {
    throw new Error('conclusion zh missing version/dir text');
  }
  if (finalHtml.includes('__VERSION__') || finalHtml.includes('__EXTENSION_DIR__')) {
    throw new Error('onboarding placeholders not fully substituted');
  }
  // §1.3 包扫描口径：无远端脚本/eval/localhost/Native Port/开发机路径。
  const forbidden = [/https?:\/\//, /\beval\s*\(/, /localhost/i, /chrome\.runtime/, /\/Users\//, /<a\s+href/i];
  for (const pattern of forbidden) {
    if (pattern.test(finalHtml)) throw new Error(`onboarding contains forbidden content: ${pattern}`);
  }

  // 编译并核验统一 App Runtime 二进制（ADR-0031：所有官方内置应用编译进唯一 app-runtime）
  const appRuntimeBin = buildAppRuntimeBinary(mode);
  const appRuntimeSha256 = digest(readFileSync(appRuntimeBin));
  const appRuntimeBytes = statSync(appRuntimeBin).size;

// 官方内置模块清单：唯一来源 modules/registry.json + module.json（ADR-0031 §8）。
  // Installer 不硬编码模块特例；新增官方模块只需改 registry。
  const registry = JSON.parse(readFileSync(resolve(ROOT, 'modules/registry.json'), 'utf8'));
  const modules = registry.apps.map((entry) => {
    const meta = JSON.parse(readFileSync(resolve(ROOT, entry.manifest), 'utf8'));
    if (meta.appId !== entry.appId) {
      throw new Error(`registry appId ${entry.appId} != module.json appId ${meta.appId}`);
    }
    const uiDist = resolve(ROOT, 'modules', entry.appId, 'ui/dist');
    const uiDst = join(STAGING, 'modules', entry.appId, 'ui');
    mkdirSync(uiDst, { recursive: true });
    if (!existsSync(join(uiDist, 'index.html'))) {
      throw new Error(`module ${entry.appId} UI missing: modules/${entry.appId}/ui/dist/index.html (run module UI build first)`);
    }
    copyFileSync(join(uiDist, 'index.html'), join(uiDst, 'index.html'));
    return {
      appId: meta.appId,
      entryRoute: meta.entryRoute,
      moduleApiVersion: meta.moduleApiVersion,
      dataSchemaVersion: meta.dataSchemaVersion,
      capabilityVersion: meta.capabilityVersion,
      ui: {
        path: `modules/${meta.appId}/ui`,
        treeSha256: treeSha256(uiDst),
      },
    };
  });

  // 产品组合清单 Schema 2（ADR-0031）：
  // 声明唯一 appRuntime 与各内置模块元数据（UI 资源树、协议契约），不声明独立可执行文件
  const arch = rustTargetArch();
  const manifest = {
    schemaVersion: 2,
    product: 'natives',
    version,
    platform: 'darwin',
    arch,
    launcher: {
      bundleId: mode === 'local' ? 'com.natives.local.app' : 'com.natives.app',
      executableSha256: digest(readFileSync(appExec)),
      onboardingSha256: digest(readFileSync(join(onboardingDir, 'index.html'))),
    },
    extension: {
      path: 'ChromeExtension',
      treeSha256: treeSha256(join(ROOT, 'dist/extension')),
    },
    appRuntime: {
      protocolVersion: 2,
      path: 'hosts/natives-app-runtime',
      bytes: appRuntimeBytes,
      sha256: appRuntimeSha256,
    },
    modules,
  };
  const manifestBytes = Buffer.from(JSON.stringify(manifest, null, 2) + '\n');
  const manifestPath = join(STAGING, 'product-manifest.json');
  writeFileSync(manifestPath, manifestBytes);
  const keyPath = manifestKey
    || (mode === 'production'
      ? null
      : resolve(ROOT, 'scripts/apps/keys/product-trust-dev.private.pem'));
  if (!keyPath) throw new Error('production requires --manifest-key (production product signing key)');
  if (!existsSync(keyPath)) throw new Error(`missing product signing key: ${keyPath}`);
  const signature = signProductManifest(manifestBytes, keyPath);
  writeFileSync(`${manifestPath}.sig`, signature + '\n');

  auditTree(STAGING);

  output = output || join(ROOT, 'dist/installer', `Natives-${version}-macOS-${arch}-${mode}.pkg`);
  const engine = join(ROOT, 'installers/macos/build-pkg.sh');
  const args = ['installers/macos/build-pkg.sh',
    '--mode', mode, '--host', host, '--model-host', modelHost, '--app-runtime', appRuntimeBin,
    ...(pkgSign ? ['--pkg-sign', pkgSign] : []),
    ...(productSign ? ['--product-sign', productSign] : []),
    '--extension-id', extensionId, '--extension-dir', join(ROOT, 'dist/extension'),
    '--modules-dir', join(STAGING, 'modules'),
    '--product-manifest', manifestPath,
    '--app-exec', appExec, '--app-icon', appIcon,
    '--onboarding-dir', onboardingDir, '--conclusion-dir', conclusionDir,
    '--version', version, '--output', output];
  execFileSync('sh', args, { stdio: 'inherit', cwd: ROOT });
  const pkgBytes = readFileSync(output);
  const sums = [
    `${digest(pkgBytes)}  ${basename(output)}`,
    `${appRuntimeSha256}  hosts/natives-app-runtime`,
  ].join('\n') + '\n';
  writeFileSync(join(dirname(output), 'SHA256SUMS'), sums);

  // 分发外壳（用户决定 2026-09-13）：DMG 内含同一个已核验 .pkg 安装器、
  // SHA256SUMS 与首次安装说明；安装内容与引擎不变（写 /Library 仍由
  // pkg 完成）。DMG 是 UDZO 只读压缩镜像。
  const dmgStaging = join(STAGING, 'dmg');
  mkdirSync(dmgStaging, { recursive: true });
  copyFileSync(output, join(dmgStaging, basename(output)));
  copyFileSync(join(dirname(output), 'SHA256SUMS'), join(dmgStaging, 'SHA256SUMS'));
  // §1.4：说明只写用户三步；技术细节放开发文档。
  writeFileSync(join(dmgStaging, '安装说明.txt'), [
    `Natives ${version}（macOS ${arch}）`,
    '',
    '1. 双击其中的 .pkg → 继续 → 输入管理员密码完成安装。',
    '2. 打开"应用程序"，双击 Natives，按引导在 Chrome 加载扩展。',
    '3. 加载后点击原生窗口的"重新检测"即可进入 Natives。',
    '',
    '卸载：在"应用程序"旁卸载工具或随包卸载脚本执行；个人数据保留。',
    '',
  ].join('\n') + '\n');
  const dmgPath = output.replace(/\.pkg$/, '.dmg');
  execFileSync('hdiutil', ['create', '-volname', `Natives ${version}`, '-srcfolder', dmgStaging,
    '-ov', '-format', 'UDZO', dmgPath], { stdio: 'inherit' });
  const dmgBytes = readFileSync(dmgPath);
  writeFileSync(join(dirname(output), 'SHA256SUMS'), [
    sums.trimEnd(),
    `${digest(dmgBytes)}  ${basename(dmgPath)}`,
  ].join('\n') + '\n');
  return { pkg: output, dmg: dmgPath, sha256: digest(pkgBytes), dmgSha256: digest(dmgBytes), size: pkgBytes.length, extensionId, manifest };
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const args = process.argv.slice(2);
  const options = {};
  for (let i = 0; i < args.length; i += 1) {
    if (args[i] === '--mode') options.mode = args[++i];
    else if (args[i] === '--version') options.version = args[++i];
    else if (args[i] === '--output') options.output = args[++i];
    else if (args[i] === '--manifest-key') options.manifestKey = args[++i];
    else if (args[i] === '--pkg-sign') options.pkgSign = args[++i];
    else if (args[i] === '--product-sign') options.productSign = args[++i];
  }
  buildInstaller(options).then((result) => {
    console.log(JSON.stringify({ pkg: result.pkg, dmg: result.dmg, sha256: result.sha256, dmgSha256: result.dmgSha256, size: result.size, extensionId: result.extensionId }));
  }).catch((error) => {
    console.error(error.message || error);
    process.exit(1);
  });
}
