import { spawnSync } from 'node:child_process';
import { cp, mkdir, readFile, rm, writeFile, stat, readdir } from 'node:fs/promises';
import { buildExtension } from './extension-package.mjs';
import { existsSync, readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve, basename, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const EXTENSION_SRC = join(ROOT, 'extension');
const DIST_DIR = join(ROOT, 'dist');
const STAGING_DIR = join(DIST_DIR, 'Natives-Windows-x64-Test');
const ZIP_PATH = join(DIST_DIR, 'Natives-Windows-x64-Test.zip');
const TEST_KEY_PATH = join(ROOT, 'installers', 'test-extension-public-key.txt');

const WINDOWS_TARGET = 'x86_64-pc-windows-gnu';
const NATIVE_HOST_EXE = join(ROOT, 'target', WINDOWS_TARGET, 'release', 'native-file-host.exe');
const MODEL_HOST_EXE = join(ROOT, 'target', WINDOWS_TARGET, 'release', 'model-host.exe');

console.log('=======================================================');
console.log('  Natives Windows x64 内测体验包打包程序 (macOS 本地)');
console.log('=======================================================\n');

// 1. Check prerequisites
function checkPrerequisites() {
  console.log('[1/7] 检查构建环境与工具链...');

  // Check mingw-w64 gcc
  const gccCheck = spawnSync('which', ['x86_64-w64-mingw32-gcc'], { encoding: 'utf8' });
  if (gccCheck.status !== 0 || !gccCheck.stdout.trim()) {
    console.error('\n❌ 错误: 未找到 mingw-w64 Windows 交叉编译器 (x86_64-w64-mingw32-gcc)');
    console.error('请在终端运行以下命令进行安装:');
    console.error('  brew install mingw-w64\n');
    process.exit(1);
  }

  // Check rust target
  const rustTargetCheck = spawnSync('rustup', ['target', 'list', '--installed'], { encoding: 'utf8' });
  if (!rustTargetCheck.stdout.includes(WINDOWS_TARGET)) {
    console.error(`\n❌ 错误: Rust 未安装 ${WINDOWS_TARGET} 目标架构`);
    console.error('请在终端运行以下命令进行安装:');
    console.error(`  rustup target add ${WINDOWS_TARGET}\n`);
    process.exit(1);
  }

  // Check Go
  const goCheck = spawnSync('go', ['version'], { encoding: 'utf8' });
  if (goCheck.status !== 0) {
    console.error('\n❌ 错误: 未找到 Go 编译器');
    process.exit(1);
  }

  // Check Cargo
  const cargoCheck = spawnSync('cargo', ['--version'], { encoding: 'utf8' });
  if (cargoCheck.status !== 0) {
    console.error('\n❌ 错误: 未找到 Cargo 编译器');
    process.exit(1);
  }

  console.log('  √ mingw-w64 gcc、rustup x86_64-pc-windows-gnu、Go、Cargo 均已就绪\n');
}

// 2. Extension ID calculation
function computeExtensionId(publicKeyBase64) {
  const der = Buffer.from(publicKeyBase64.trim(), 'base64');
  const sha = createHash('sha256').update(der).digest();
  return sha.subarray(0, 16).toString('hex').replace(/[0-9a-f]/g, (n) => String.fromCharCode(97 + parseInt(n, 16)));
}

// Helper to run commands
function run(command, args, options = {}, failureMsg = 'Command failed') {
  const result = spawnSync(command, args, { cwd: ROOT, stdio: 'inherit', ...options });
  if (result.status !== 0) {
    console.error(`\n❌ ${failureMsg}`);
    process.exit(1);
  }
}

// Verify Windows PE32+ (x86-64) format
function verifyPE32Plus(filePath, label) {
  const buffer = readFileSync(filePath);
  if (buffer.length < 64) throw new Error(`${label} 文件过小，非有效可执行文件`);
  if (buffer[0] !== 0x4D || buffer[1] !== 0x5A) throw new Error(`${label} 缺少 MZ 头`);
  const peOffset = buffer.readUInt32LE(0x3C);
  if (buffer.length < peOffset + 26) throw new Error(`${label} 缺少 PE 头`);
  if (buffer[peOffset] !== 0x50 || buffer[peOffset + 1] !== 0x45 || buffer[peOffset + 2] !== 0 || buffer[peOffset + 3] !== 0) {
    throw new Error(`${label} 缺少 PE 签名`);
  }
  const machine = buffer.readUInt16LE(peOffset + 4);
  if (machine !== 0x8664) throw new Error(`${label} 机器类型不是 x86-64 (0x8664)，读取到 0x${machine.toString(16)}`);
  const magic = buffer.readUInt16LE(peOffset + 24);
  if (magic !== 0x020B) throw new Error(`${label} 可选头魔数不是 PE32+ (0x020B)，读取到 0x${magic.toString(16)}`);
}

async function main() {
  checkPrerequisites();

  // Load / verify public key
  if (!existsSync(TEST_KEY_PATH)) {
    console.error(`\n❌ 错误: 缺少测试公钥文件 ${TEST_KEY_PATH}`);
    process.exit(1);
  }
  const testPublicKey = (await readFile(TEST_KEY_PATH, 'utf8')).trim();
  const extensionId = computeExtensionId(testPublicKey);
  console.log(`[2/7] 锁定固定测试公钥与扩展 ID: ${extensionId}`);

  // 3. Run test suites
  console.log('\n[3/7] 执行全量单元测试与语法检查...');
  run('npm', ['run', 'extension:check'], {}, '扩展代码与插件语法/行为测试失败');
  run('cargo', ['test', '--workspace'], {}, 'Rust 单元测试失败');
  run('go', ['test', './...'], { cwd: join(ROOT, 'model-host') }, 'Go 单元测试失败');

  // 4. Compile Windows Hosts
  console.log('\n[4/7] 交叉编译 Windows x64 原生可执行文件...');
  console.log('  -> 编译 native-file-host.exe (Rust)...');
  run('cargo', ['build', '-p', 'native-file-host', '--release', '--target', WINDOWS_TARGET], {}, 'native-file-host 编译失败');

  console.log('  -> 编译 model-host.exe (Go)...');
  run('go', ['build', '-trimpath', '-ldflags=-s -w', '-o', MODEL_HOST_EXE, '.'], {
    cwd: join(ROOT, 'model-host'),
    env: { ...process.env, CGO_ENABLED: '0', GOOS: 'windows', GOARCH: 'amd64' },
  }, 'model-host 编译失败');

  // Verify PE formats
  verifyPE32Plus(NATIVE_HOST_EXE, 'native-file-host.exe');
  verifyPE32Plus(MODEL_HOST_EXE, 'model-host.exe');
  console.log('  √ 验证成功：两个二进制文件均为标准 Windows PE32+ (x86-64) 可执行文件');

  // 5. Assemble package
  console.log('\n[5/7] 组装 Windows 体验包文件结构...');
  await rm(STAGING_DIR, { recursive: true, force: true });
  await mkdir(join(STAGING_DIR, 'Extension'), { recursive: true });
  await mkdir(join(STAGING_DIR, 'Host'), { recursive: true });

  // Copy Extension production files
  const excludedNames = new Set([
    'README.md', 'check-lifecycle.mjs', 'check-syntax.mjs', 'dev.mjs',
    'install-native-host.mjs', 'launch-workbench.mjs', 'native-host-manifest.json',
    'folder-source.svg', 'test-dom-mock.js',
  ]);

  buildExtension(join(STAGING_DIR, 'Extension'));

  // Inject fixed public key into manifest.json
  const manifestPath = join(STAGING_DIR, 'Extension', 'manifest.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  manifest.key = testPublicKey;
  await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + '\n');

  // Copy Host EXEs
  await cp(NATIVE_HOST_EXE, join(STAGING_DIR, 'Host', 'native-file-host.exe'));
  await cp(MODEL_HOST_EXE, join(STAGING_DIR, 'Host', 'model-host.exe'));

  // Batch command content
  const installCmd = `@echo off
chcp 65001 >nul
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\\install-host.ps1"
pause
`;

  const launchCmd = `@echo off
chcp 65001 >nul
start chrome.exe "chrome-extension://${extensionId}/space.html"
`;

  const uninstallCmd = `@echo off
chcp 65001 >nul
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\\uninstall-host.ps1"
pause
`;

  const checkCmd = `@echo off
chcp 65001 >nul
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\\self-check.ps1"
pause
`;

  await writeFile(join(STAGING_DIR, '安装并注册 Host.cmd'), installCmd);
  await writeFile(join(STAGING_DIR, 'install.cmd'), installCmd);

  await writeFile(join(STAGING_DIR, '启动 Natives.cmd'), launchCmd);
  await writeFile(join(STAGING_DIR, 'launch.cmd'), launchCmd);

  await writeFile(join(STAGING_DIR, '卸载 Natives.cmd'), uninstallCmd);
  await writeFile(join(STAGING_DIR, 'uninstall.cmd'), uninstallCmd);

  await writeFile(join(STAGING_DIR, '检查安装状态.cmd'), checkCmd);
  await writeFile(join(STAGING_DIR, 'check-status.cmd'), checkCmd);

  // install-host.ps1
  await writeFile(join(STAGING_DIR, 'install-host.ps1'), `# [Natives] Windows 内测体验包安装与注册脚本
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "  Natives Windows 体验包安装与 Native Host 注册程序" -ForegroundColor Cyan
Write-Host "=======================================================\`n"

# 1. 检查 Chrome
$chromePaths = @(
    (Join-Path $env:LOCALAPPDATA "Google\\Chrome\\Application\\chrome.exe"),
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe"
)

$foundChrome = $null
foreach ($path in $chromePaths) {
    if (Test-Path $path) {
        $foundChrome = $path
        break
    }
}

if (-not $foundChrome) {
    Write-Host "[错误] 未检测到 Google Chrome 浏览器！" -ForegroundColor Red
    Write-Host "Natives 依赖 Google Chrome 运行，请先安装 Google Chrome 后重试。" -ForegroundColor Yellow
    Exit 1
}

Write-Host "[1/4] 检测到 Google Chrome: $foundChrome" -ForegroundColor Green

# 2. 复制文件到 %LOCALAPPDATA%\\Natives-Test
$targetDir = Join-Path $env:LOCALAPPDATA "Natives-Test"
Write-Host "[2/4] 正在安装文件到: $targetDir ..." -ForegroundColor Cyan

if (Test-Path $targetDir) {
    # 停止旧进程
    Stop-Process -Name "native-file-host", "model-host" -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
}

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $targetDir "manifests") | Out-Null

$scriptDir = $PSScriptRoot
Copy-Item -Path (Join-Path $scriptDir "Extension") -Destination $targetDir -Recurse -Force
Copy-Item -Path (Join-Path $scriptDir "Host") -Destination $targetDir -Recurse -Force
Copy-Item -Path (Join-Path $scriptDir "uninstall-host.ps1") -Destination $targetDir -Force
Copy-Item -Path (Join-Path $scriptDir "self-check.ps1") -Destination $targetDir -Force

# 3. 生成 Manifest 并写入注册表
Write-Host "[3/4] 正在注册 Chrome Native Messaging Hosts..." -ForegroundColor Cyan

$extensionId = "${extensionId}"
$fileHostExe = (Join-Path $targetDir "Host\\native-file-host.exe").Replace('\\', '\\\\')
$modelHostExe = (Join-Path $targetDir "Host\\model-host.exe").Replace('\\', '\\\\')

$fileManifestContent = @'
{
  "name": "com.natives.file_manager",
  "description": "Natives Chromium File Manager Host",
  "path": "@FILE_HOST_EXE@",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://@EXT_ID@/"
  ]
}
'@.Replace('@FILE_HOST_EXE@', $fileHostExe).Replace('@EXT_ID@', $extensionId)

$modelManifestContent = @'
{
  "name": "com.natives.model_host",
  "description": "Natives model settings and local model proxy",
  "path": "@MODEL_HOST_EXE@",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://@EXT_ID@/"
  ]
}
'@.Replace('@MODEL_HOST_EXE@', $modelHostExe).Replace('@EXT_ID@', $extensionId)

$fileManifestPath = Join-Path $targetDir "manifests\\com.natives.file_manager.json"
$modelManifestPath = Join-Path $targetDir "manifests\\com.natives.model_host.json"

[System.IO.File]::WriteAllText($fileManifestPath, $fileManifestContent, [System.Text.Encoding]::UTF8)
[System.IO.File]::WriteAllText($modelManifestPath, $modelManifestContent, [System.Text.Encoding]::UTF8)

# 写入当前用户注册表
$regBasePath = "HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts"
New-Item -Path "$regBasePath\\com.natives.file_manager" -Force | Out-Null
Set-ItemProperty -Path "$regBasePath\\com.natives.file_manager" -Name "(Default)" -Value $fileManifestPath -Force

New-Item -Path "$regBasePath\\com.natives.model_host" -Force | Out-Null
Set-ItemProperty -Path "$regBasePath\\com.natives.model_host" -Name "(Default)" -Value $modelManifestPath -Force

# 4. 创建桌面快捷方式
$desktopPath = [Environment]::GetFolderPath("Desktop")
$shortcutPath = Join-Path $desktopPath "启动 Natives.lnk"

$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut($shortcutPath)
$Shortcut.TargetPath = $foundChrome
$Shortcut.Arguments = "chrome-extension://$extensionId/space.html"
$Shortcut.Description = "启动 Natives 个人空间"
$Shortcut.Save()

Write-Host "[4/4] 注册表与桌面快捷方式创建完成！" -ForegroundColor Green

# 5. 打开 Chrome 扩展页面与目录
Write-Host "\`n正在启动 Chrome 扩展管理页面并打开 Extension 文件夹..." -ForegroundColor Cyan
Start-Process $foundChrome "chrome://extensions"
Start-Process "explorer.exe" (Join-Path $targetDir "Extension")

Write-Host "\`n=======================================================" -ForegroundColor Yellow
Write-Host "  [√] 安装与注册完成！" -ForegroundColor Green
Write-Host "  请在刚刚弹出的 Chrome 扩展页面中执行以下最后一步：" -ForegroundColor White
Write-Host "  1. 开启右上角的【开发者模式】" -ForegroundColor White
Write-Host "  2. 点击左上角的【加载已解压的扩展程序】" -ForegroundColor White
Write-Host "  3. 选择刚刚弹出的 Extension 文件夹：" -ForegroundColor White
Write-Host "     $(Join-Path $targetDir 'Extension')" -ForegroundColor Cyan
Write-Host "=======================================================\`n" -ForegroundColor Yellow
`);

  // uninstall-host.ps1
  await writeFile(join(STAGING_DIR, 'uninstall-host.ps1'), `# [Natives] Windows 内测体验包卸载脚本
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "  Natives Windows 体验包卸载程序" -ForegroundColor Cyan
Write-Host "=======================================================\`n"

# 1. 停止进程
Write-Host "[1/4] 停止正在运行的 Native Host 进程..." -ForegroundColor Cyan
Stop-Process -Name "native-file-host", "model-host" -ErrorAction SilentlyContinue

# 2. 清理注册表
Write-Host "[2/4] 清理 Chrome Native Messaging 注册表项..." -ForegroundColor Cyan
$regBasePath = "HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts"
Remove-Item -Path "$regBasePath\\com.natives.file_manager" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "$regBasePath\\com.natives.model_host" -Recurse -Force -ErrorAction SilentlyContinue

# 3. 清理桌面快捷方式
Write-Host "[3/4] 清理桌面快捷方式..." -ForegroundColor Cyan
$desktopPath = [Environment]::GetFolderPath("Desktop")
$shortcutPath = Join-Path $desktopPath "启动 Natives.lnk"
if (Test-Path $shortcutPath) {
    Remove-Item -Path $shortcutPath -Force -ErrorAction SilentlyContinue
}

# 4. 删除安装目录
Write-Host "[4/4] 删除已安装文件..." -ForegroundColor Cyan
$targetDir = Join-Path $env:LOCALAPPDATA "Natives-Test"
if (Test-Path $targetDir) {
    Remove-Item -Path $targetDir -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "\`n=======================================================" -ForegroundColor Green
Write-Host "  [√] Natives Host 卸载完成！" -ForegroundColor Green
Write-Host "  请前往 Chrome 浏览器 chrome://extensions 页面手动移除 Natives 扩展。" -ForegroundColor Yellow
Write-Host "=======================================================\`n"
`);

  // self-check.ps1
  await writeFile(join(STAGING_DIR, 'self-check.ps1'), `# [Natives] Windows 安装状态诊断脚本
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "  Natives Windows 安装状态诊断报告" -ForegroundColor Cyan
Write-Host "=======================================================\`n"

$targetDir = Join-Path $env:LOCALAPPDATA "Natives-Test"
$regBasePath = "HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts"

Write-Host "1. 安装目录检查:"
if (Test-Path $targetDir) {
    Write-Host "  [√] 安装目录存在: $targetDir" -ForegroundColor Green
} else {
    Write-Host "  [×] 安装目录未找到: $targetDir" -ForegroundColor Red
}

Write-Host "\`n2. 可执行文件检查:"
$fileExe = Join-Path $targetDir "Host\\native-file-host.exe"
$modelExe = Join-Path $targetDir "Host\\model-host.exe"

if (Test-Path $fileExe) {
    Write-Host "  [√] native-file-host.exe 存在" -ForegroundColor Green
} else {
    Write-Host "  [×] native-file-host.exe 未找到" -ForegroundColor Red
}

if (Test-Path $modelExe) {
    Write-Host "  [√] model-host.exe 存在" -ForegroundColor Green
} else {
    Write-Host "  [×] model-host.exe 未找到" -ForegroundColor Red
}

Write-Host "\`n3. 注册表与 Manifest 关联检查:"
$fileReg = (Get-ItemProperty -Path "$regBasePath\\com.natives.file_manager" -ErrorAction SilentlyContinue)."(Default)"
$modelReg = (Get-ItemProperty -Path "$regBasePath\\com.natives.model_host" -ErrorAction SilentlyContinue)."(Default)"

if ($fileReg -and (Test-Path $fileReg)) {
    Write-Host "  [√] com.natives.file_manager 注册有效: $fileReg" -ForegroundColor Green
} else {
    Write-Host "  [×] com.natives.file_manager 注册缺失或文件不存在" -ForegroundColor Red
}

if ($modelReg -and (Test-Path $modelReg)) {
    Write-Host "  [√] com.natives.model_host 注册有效: $modelReg" -ForegroundColor Green
} else {
    Write-Host "  [×] com.natives.model_host 注册缺失或文件不存在" -ForegroundColor Red
}

Write-Host "\`n=======================================================" -ForegroundColor Cyan
`);

  // EXTENSION-ID.txt
  await writeFile(join(STAGING_DIR, 'EXTENSION-ID.txt'), `${extensionId}\n`);

  // 使用说明.txt & README.txt
  const readmeContent = `=======================================================
  Natives (Windows x64 内部体验版) 使用说明
=======================================================

【安装步骤】
1. 解压此 ZIP 压缩包至任意文件夹。
2. 双击运行【安装并注册 Host.cmd】(或 install.cmd)。
3. 安装程序会自动：
   - 将运行文件复制到 %LOCALAPPDATA%\\Natives-Test
   - 注册 Chrome Native Messaging 桥接服务
   - 打开 Chrome 扩展程序管理页面 (chrome://extensions)
   - 弹出已就绪的 Extension 文件夹
4. 在 Chrome 扩展页面中：
   - 开启右上角的【开发者模式】
   - 点击左上角【加载已解压的扩展程序】
   - 选择弹出的 Extension 文件夹
5. 打开 Chrome 新标签页，即可体验 Natives 个人空间与文件管理！

【日常启动】
- 打开 Chrome 新标签页即可进入个人空间。
- 或双击桌面的【启动 Natives】快捷方式。

【更新体验包】
- 解压新版本 ZIP，再次双击【安装并注册 Host.cmd】覆盖安装。
- 在 chrome://extensions 中找到 Natives 点击【重新加载】图标。

【卸载】
- 双击运行【卸载 Natives.cmd】(或 uninstall.cmd) 即可完全清理注册表与本地文件。
- 在 Chrome 扩展管理页面移除 Natives 扩展。
`;
  await writeFile(join(STAGING_DIR, '使用说明.txt'), readmeContent);
  await writeFile(join(STAGING_DIR, 'README.txt'), readmeContent);

  // 10. Compute SHA-256 for all files
  console.log('\n[6/7] 生成文件 SHA-256 校验和...');
  async function getFilesRecursively(dir) {
    const entries = await readdir(dir, { withFileTypes: true });
    const files = [];
    for (const entry of entries) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        files.push(...await getFilesRecursively(full));
      } else {
        files.push(full);
      }
    }
    return files;
  }

  const allStagingFiles = await getFilesRecursively(STAGING_DIR);
  const checksumLines = [];
  for (const file of allStagingFiles) {
    const rel = relative(STAGING_DIR, file).replace(/\\/g, '/');
    const content = await readFile(file);
    const hash = createHash('sha256').update(content).digest('hex');
    checksumLines.push(`${hash}  ${rel}`);
  }
  await writeFile(join(STAGING_DIR, 'SHA256SUMS.txt'), checksumLines.join('\n') + '\n');

  // 7. Zip package
  console.log('\n[7/7] 打包 ZIP 压缩包 (排除系统脏文件)...');
  await rm(ZIP_PATH, { force: true });
  run('zip', ['-r', '-X', '-q', ZIP_PATH, 'Natives-Windows-x64-Test'], { cwd: DIST_DIR }, '创建 ZIP 压缩包失败');

  const zipStat = await stat(ZIP_PATH);
  const zipHash = createHash('sha256').update(await readFile(ZIP_PATH)).digest('hex');

  console.log('\n=======================================================');
  console.log('  🎉 Windows x64 内测体验包构建完成！');
  console.log('=======================================================');
  console.log(`  输出文件 : ${ZIP_PATH}`);
  console.log(`  文件大小 : ${(zipStat.size / 1024 / 1024).toFixed(2)} MB`);
  console.log(`  SHA-256  : ${zipHash}`);
  console.log(`  扩展 ID  : ${extensionId}`);
  console.log('=======================================================\n');
}

main().catch((err) => {
  console.error('\n❌ 打包过程发生异常:', err);
  process.exit(1);
});
