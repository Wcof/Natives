#!/usr/bin/env node
// Diagnose Native Messaging registration for every Chromium-family browser and
// the two known Natives extension identities (dev and local candidate).
import { existsSync, readFileSync } from 'node:fs';
import { homedir, platform } from 'node:os';
import { join } from 'node:path';
import { STABLE_EXTENSION_ID, LOCAL_EXTENSION_ID } from './extension-package.mjs';

const home = homedir();
const hosts = ['com.natives.file_manager', 'com.natives.model_host', 'com.natives.local.app_runtime'];
const browsers = platform() === 'darwin'
  ? ['Google/Chrome', 'Google/Chrome for Testing', 'Chromium', 'BraveSoftware/Brave-Browser', 'Microsoft Edge']
  : platform() === 'win32'
    ? ['Google/Chrome', 'Google/Chrome for Testing', 'Chromium', 'BraveSoftware/Brave-Browser', 'Microsoft/Edge']
    : ['google-chrome', 'google-chrome-beta', 'chromium', 'brave-browser', 'microsoft-edge'];
function manifestPath(browser, host) {
  if (platform() === 'darwin') return join(home, 'Library/Application Support', browser, 'NativeMessagingHosts', `${host}.json`);
  if (platform() === 'win32') return join(process.env.LOCALAPPDATA || join(home, 'AppData/Local'), 'Natives', `${host}.json`);
  return join(home, `.config/${browser}/NativeMessagingHosts/${host}.json`);
}
const expected = [`chrome-extension://${STABLE_EXTENSION_ID}/`, `chrome-extension://${LOCAL_EXTENSION_ID}/`];
const report = { platform: platform(), expectedOrigins: expected, browsers: [] };
let failures = 0;
for (const browser of browsers) {
  const entry = { browser, hosts: [] };
  for (const host of hosts) {
    const path = manifestPath(browser, host);
    const item = { host, path, exists: existsSync(path) };
    if (item.exists) {
      try {
        const manifest = JSON.parse(readFileSync(path, 'utf8'));
        item.name = manifest.name;
        item.pathExists = typeof manifest.path === 'string' && existsSync(manifest.path);
        item.allowedOrigins = manifest.allowed_origins;
        item.originsOk = expected.every(origin => Array.isArray(manifest.allowed_origins) && manifest.allowed_origins.includes(origin));
        item.ok = item.name === host && manifest.type === 'stdio' && item.pathExists && item.originsOk;
      } catch (error) {
        item.error = error.message;
        item.ok = false;
      }
    } else item.ok = false;
    if (!item.ok) failures += 1;
    entry.hosts.push(item);
  }
  report.browsers.push(entry);
}
report.ok = failures === 0;
console.log(JSON.stringify(report, null, 2));
if (!report.ok) process.exitCode = 1;
