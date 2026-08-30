import { mkdir, readFile, unlink, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { homedir, platform } from 'node:os';
import { dirname, join, resolve } from 'node:path';

const args = new Map(process.argv.slice(2).flatMap((value, index, values) =>
  value.startsWith('--') ? [[value.slice(2), values[index + 1]]] : []));
const uninstall = process.argv.includes('--uninstall');
const extensionId = args.get('extension-id');
const hostPath = args.get('host-path');
if (!uninstall && (!extensionId || !hostPath || !/^[a-p]{32}$/.test(extensionId))) {
  console.error('用法：node extension/install-native-host.mjs --extension-id <32位ID> --host-path <Host绝对路径>');
  process.exit(1);
}

const template = JSON.parse(await readFile(new URL('./native-host-manifest.json', import.meta.url), 'utf8'));
if (!uninstall) {
  template.allowed_origins = [`chrome-extension://${extensionId}/`];
  template.path = resolve(hostPath);
}
const home = homedir();
const configPath = platform() === 'darwin'
  ? join(home, 'Library/Application Support/Natives/extension-id')
  : platform() === 'win32'
    ? join(process.env.LOCALAPPDATA || join(home, 'AppData/Local'), 'Natives/extension-id')
    : join(home, '.config/natives/extension-id');
const destinations = platform() === 'darwin'
  ? ['Google/Chrome', 'Chromium'].map(browser => join(home, `Library/Application Support/${browser}/NativeMessagingHosts/com.natives.file_manager.json`))
  : platform() === 'win32'
    ? [join(process.env.LOCALAPPDATA || join(home, 'AppData/Local'), 'Natives/com.natives.file_manager.json')]
    : ['google-chrome', 'chromium'].map(browser => join(home, `.config/${browser}/NativeMessagingHosts/com.natives.file_manager.json`));
if (uninstall) {
  await Promise.all(destinations.map(path => unlink(path).catch(() => {})));
  await unlink(configPath).catch(() => {});
  if (platform() === 'win32') {
    for (const browser of ['Google\\Chrome', 'Chromium']) {
      execFileSync('reg.exe', ['DELETE', `HKCU\\Software\\${browser}\\NativeMessagingHosts\\com.natives.file_manager`, '/f'], { stdio: 'inherit' });
    }
  }
  console.log(`已卸载 Native Host：${destinations.join('、')}`);
  process.exit(0);
}
for (const destination of destinations) {
  await mkdir(dirname(destination), { recursive: true });
  await writeFile(destination, `${JSON.stringify(template, null, 2)}\n`, 'utf8');
}
await mkdir(dirname(configPath), { recursive: true });
await writeFile(configPath, `${extensionId}\n`, 'utf8');
if (platform() === 'win32') {
  for (const browser of ['Google\\Chrome', 'Chromium']) {
    execFileSync('reg.exe', [
      'ADD', `HKCU\\Software\\${browser}\\NativeMessagingHosts\\com.natives.file_manager`,
      '/ve', '/t', 'REG_SZ', '/d', destinations[0], '/f',
    ], { stdio: 'inherit' });
  }
}
console.log(`已注册 Native Host：${destinations.join('、')}`);
