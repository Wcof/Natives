import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { join, resolve } from 'node:path';
import { ROOT } from '../extension-package.mjs';
import { resolveAppPackages } from '../../extension/app-catalog-policy.js';

const platform = process.platform === 'win32' ? 'windows' : process.platform;
const arch = process.arch === 'x64' ? 'x64' : process.arch;
const filename = process.platform === 'win32' ? 'demo-host.exe' : 'demo-host';
const binary = join(ROOT, 'target/release', filename);
const health = JSON.parse(execFileSync(binary, ['--health'], { timeout: 3000, encoding: 'utf8' }));
if (health.status !== 'ok' || !/^\d+\.\d+\.\d+$/.test(health.version)) throw new Error('invalid Demo release health');
const payload = readFileSync(binary), nap = gzipSync(payload, { level: 9 });
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const artifact = `demo-host-${platform}-${arch}-${health.version}.nap`;
const entry = {
  app_id: 'com.natives.app.demo', kind: 'extension_app', name: 'Demo', version: health.version,
  minNativesVersion: '0.1.0', description: { zh_CN: 'Native Messaging 运行状态', en: 'Native Messaging runtime status' },
  icon: 'grid', permissions: [], runtime_spec: { host: 'com.natives.app.demo', version: health.version },
  surface: { icon: 'grid', route: 'app.html?app=com.natives.app.demo' }, manifest: { permissions: [] },
  packages: [{ package_id: filename, kind: 'runtime', version: health.version, platform, arch, required: true,
    url: `https://github.com/Wcof/Natives/releases/download/apps-demo-v${health.version}/${artifact}`,
    wire_size: nap.length, payload_size: payload.length, artifact_sha256: digest(nap), payload_sha256: digest(payload) }],
};
const resolved = resolveAppPackages(entry, { platform, arch, version: '0.1.0' });
if (resolved.reason) throw new Error(resolved.reason);
const output = resolve(ROOT, process.argv[2] || 'dist/app-release');
mkdirSync(output, { recursive: true });
writeFileSync(join(output, artifact), nap);
writeFileSync(join(output, `catalog-${platform}-${arch}.json`), JSON.stringify(entry, null, 2) + '\n');
console.log(JSON.stringify({ artifact: join(output, artifact), wireBytes: nap.length, payloadBytes: payload.length, version: health.version }));
