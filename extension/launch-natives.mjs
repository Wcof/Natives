import { existsSync, readFileSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { homedir, platform } from 'node:os';
import { join } from 'node:path';

const home = homedir();
const configPath = platform() === 'darwin'
  ? join(home, 'Library/Application Support/Natives/extension-id')
  : platform() === 'win32'
    ? join(process.env.LOCALAPPDATA || join(home, 'AppData', 'Local'), 'Natives', 'extension-id')
    : join(home, '.config', 'natives', 'extension-id');
const configuredId = existsSync(configPath) ? readFileSync(configPath, 'utf8').trim() : '';
const extensionId = process.argv[2] || process.env.NATIVES_EXTENSION_ID || configuredId;
if (!extensionId || !/^[a-p]{32}$/.test(extensionId)) {
  console.error('请先完成扩展安装和 Native Host 注册，或传入 32 位扩展 ID。');
  process.exit(1);
}
const url = `chrome-extension://${extensionId}/files.html`;

if (platform() === 'darwin') {
  const preferred = process.env.NATIVES_BROWSER || 'Google Chrome';
  for (const browser of [preferred, preferred === 'Google Chrome' ? 'Chromium' : 'Google Chrome']) {
    if (spawnSync('open', ['-a', browser, url], { stdio: 'ignore' }).status === 0) process.exit(0);
  }
} else if (platform() === 'win32') {
  const local = process.env.LOCALAPPDATA || join(homedir(), 'AppData', 'Local');
  const candidates = [
    join(process.env.PROGRAMFILES || 'C:\\Program Files', 'Google', 'Chrome', 'Application', 'chrome.exe'),
    join(local, 'Google', 'Chrome', 'Application', 'chrome.exe'),
    join(local, 'Chromium', 'Application', 'chrome.exe'),
  ];
  const browser = candidates.find(existsSync);
  if (browser) { spawn(browser, [url], { detached: true, stdio: 'ignore' }).unref(); process.exit(0); }
} else {
  for (const browser of ['google-chrome', 'chromium', 'chromium-browser']) {
    if (spawnSync('sh', ['-c', `command -v ${browser}`], { stdio: 'ignore' }).status === 0) {
      spawn(browser, [url], { detached: true, stdio: 'ignore' }).unref(); process.exit(0);
    }
  }
}

console.error('未找到 Chrome 或 Chromium。');
process.exit(1);
