// Natives 安装包组装（用户方案 §安装包）：
//   Natives.app（Launcher 外壳：双击 → --launcher-default / --launcher-setup）
//   Contents/share/natives/Runtime/            native-file-host + model-host
//   Contents/share/natives/ChromeExtension/    natives-extension-{v}.zip + SHA256SUMS + metadata
//   → pkgbuild → Natives-{version}-macOS-{arch}.pkg
// 产物禁止：node_modules / target/debug / Git 路径 / 开发机绝对路径（自检函数 enforce）。
import { existsSync, mkdirSync, rmSync, writeFileSync, copyFileSync, readFileSync, readdirSync, symlinkSync, chmodSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const OUT = resolve(ROOT, 'dist/installer');
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

function rustTargetArch() {
  const host = execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n').find((l) => l.startsWith('host:')).split(':')[1].trim();
  return host.includes('aarch64') ? 'arm64' : 'x64';
}

// 自检：安装包树内不得出现开发环境残留（用户方案硬性禁止项）。
// 按树内相对路径匹配——绝对路径前缀（/Users/...）是安装位置，不是内容。
function auditTree(dir) {
  const forbidden = /(^|\/)(node_modules|target|\.git)(\/|$)|\/debug\/|\/Users\/[a-z]/i;
  const violations = [];
  (function visit(d, rel) {
    for (const entry of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, entry.name);
      const relPath = rel ? `${rel}/${entry.name}` : entry.name;
      if (forbidden.test(relPath)) violations.push(relPath);
      else if (entry.isDirectory()) visit(p, relPath);
    }
  })(dir, '');
  if (violations.length) throw new Error('forbidden paths in installer tree:\n' + violations.join('\n'));
  return true;
}

function buildLauncherBinary() {
  // Launcher 是 native-file-host 的薄包装：--launcher-default/--launcher-setup
  // 已在主二进制实现；app 外壳直接调用它，无需独立二进制。
  const bin = resolve(ROOT, `target/release/native-file-host`);
  if (!existsSync(bin)) throw new Error('missing target/release/native-file-host (run npm run extension:host:build:release)');
  const modelBin = resolve(ROOT, 'target/release/model-host');
  if (!existsSync(modelBin)) throw new Error('missing target/release/model-host (run npm run model:host:build:release)');
  return { coreBin: bin, modelBin };
}

export function buildInstaller({ version, arch = rustTargetArch() } = {}) {
  if (!version) {
    version = JSON.parse(readFileSync(join(ROOT, 'extension/manifest.json'), 'utf8')).version;
  }
  const extRelease = join(ROOT, 'dist/extension-release');
  const zipName = `natives-extension-${version}.zip`;
  if (!existsSync(join(extRelease, zipName))) {
    throw new Error(`missing ${zipName} (run node scripts/extension-package.mjs first)`);
  }
  const fundRelease = join(ROOT, 'dist/fund-release');
  const { coreBin, modelBin } = buildLauncherBinary();

  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(OUT, { recursive: true });

  // ---- Natives.app ----
  const app = join(OUT, 'Natives.app');
  const macos = join(app, 'Contents/MacOS');
  const resources = join(app, 'Contents/share/natives');
  const runtimeDir = join(resources, 'Runtime');
  const extDir = join(resources, 'ChromeExtension');
  mkdirSync(macos, { recursive: true });
  mkdirSync(runtimeDir, { recursive: true });
  mkdirSync(extDir, { recursive: true });

  copyFileSync(coreBin, join(macos, 'native-file-host'));
  chmodSync(join(macos, 'native-file-host'), 0o755);
  // Launcher 入口脚本：双击 → 默认模式（已就绪直接打开 Natives；
  // setup_required(2) 时转入引导流）。固定参数，不拼接外部文本。
  // 注意不能用 `exec A || exec B`：exec 替换进程后 A 的退出码不会触发 B。
  writeFileSync(join(macos, 'Natives'), [
    '#!/bin/sh',
    '# Natives Launcher：只负责 Runtime/Extension 检测、引导与打开 Natives，',
    '# 不承载任何业务 UI。',
    'DIR="$(cd "$(dirname "$0")" && pwd)"',
    '"$DIR/native-file-host" --launcher-default',
    'code=$?',
    'if [ "$code" -eq 2 ]; then',
    '  exec "$DIR/native-file-host" --launcher-setup --setup-timeout-secs 600',
    'fi',
    'exit "$code"',
    '',
  ].join('\n'));
  chmodSync(join(macos, 'Natives'), 0o755);
  copyFileSync(modelBin, join(runtimeDir, 'model-host'));
  chmodSync(join(runtimeDir, 'model-host'), 0o755);
  // Native Messaging manifest 模板：随安装包交付（用户方案硬性清单项）。
  // Launcher 首启把它写入用户级 Chrome NM 目录（allowed_origins 锁定稳定 ID）。
  const nmDir = join(resources, 'NativeMessagingManifests');
  mkdirSync(nmDir);
  for (const host of ['com.natives.file_manager', 'com.natives.model_host']) {
    const binary = host === 'com.natives.file_manager' ? 'native-file-host' : 'model-host';
    // path 占位由 Launcher 注册时以实际安装路径写入；模板记录相对布局。
    writeFileSync(join(nmDir, `${host}.json`), JSON.stringify({
      name: host,
      path: `@@INSTALL_ROOT@@/Contents/MacOS/${binary === 'model-host' ? '../share/natives/Runtime/model-host' : 'native-file-host'}`,
      type: 'stdio',
      allowed_origins: [`chrome-extension://${JSON.parse(readFileSync(join(extRelease, 'extension-metadata.json'), 'utf8')).extensionId}/`],
    }, null, 2) + '\n');
  }
  copyFileSync(join(extRelease, zipName), join(extDir, zipName));
  copyFileSync(join(extRelease, 'SHA256SUMS'), join(extDir, 'SHA256SUMS'));
  copyFileSync(join(extRelease, 'extension-metadata.json'), join(extDir, 'extension-metadata.json'));

  writeFileSync(join(app, 'Contents/Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Natives</string>
  <key>CFBundleDisplayName</key><string>Natives</string>
  <key>CFBundleIdentifier</key><string>com.natives.launcher</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleExecutable</key><string>Natives</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
`);

  // ---- release metadata + SHA256SUMS ----
  auditTree(OUT);
  const pkgBin = readFileSync(join(macos, 'native-file-host'));
  const modelBytes = readFileSync(join(runtimeDir, 'model-host'));
  const zipBytes = readFileSync(join(extDir, zipName));
  const metadata = {
    schema: 'natives-installer-metadata/1',
    version,
    arch,
    platform: 'darwin',
    extensionId: JSON.parse(readFileSync(join(extRelease, 'extension-metadata.json'), 'utf8')).extensionId,
    components: {
      nativeFileHost: { sha256: digest(pkgBin), size: pkgBin.length },
      modelHost: { sha256: digest(modelBytes), size: modelBytes.length },
      extensionZip: { sha256: digest(zipBytes), size: zipBytes.length },
    },
  };
  writeFileSync(join(OUT, 'installer-metadata.json'), JSON.stringify(metadata, null, 2) + '\n');

  // ---- pkgbuild ----
  const pkgPath = join(OUT, `Natives-${version}-macOS-${arch}.pkg`);
  execFileSync('/usr/bin/pkgbuild', [
    '--root', app,
    '--identifier', 'com.natives.launcher',
    '--version', version,
    '--install-location', '/Applications/Natives.app',
    pkgPath,
  ], { stdio: 'inherit' });

  const pkgBytes = readFileSync(pkgPath);
  writeFileSync(join(OUT, 'SHA256SUMS'), [
    `${digest(pkgBytes)}  ${basename(pkgPath)}`,
    `${digest(zipBytes)}  ${zipName}`,
  ].join('\n') + '\n');
  return { pkg: basename(pkgPath), sha256: digest(pkgBytes), size: pkgBytes.length, metadata };
}

if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  console.log(JSON.stringify(buildInstaller({})));
}
