import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { join, resolve } from 'node:path';
import { ROOT } from '../extension-package.mjs';

const version = '2.0.0';
const outputDir = resolve(ROOT, process.argv[2] || 'dist/app-release');
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const packageOf = (package_id, kind, payload, filename) => {
  const artifact = gzipSync(payload, { level: 9 });
  writeFileSync(join(outputDir, filename), artifact);
  return {
    package_id, kind, version, platform: 'any', arch: 'any', required: true,
    url: `https://github.com/Wcof/Natives/releases/download/apps-demo-v${version}/${filename}`,
    wire_size: artifact.length, payload_size: payload.length,
    artifact_sha256: digest(artifact), payload_sha256: digest(payload),
  };
};

mkdirSync(outputDir, { recursive: true });
const data = Buffer.from(JSON.stringify({
  name: 'Demo Data', version, description: '跨平台静态数据包',
  items: [{ id: 1, label: '中文示例', value: 100 }, { id: 2, label: 'Demo Item B', value: 200 }],
}, null, 2), 'utf8');
const image = readFileSync(resolve(ROOT, 'extension/icons/folder-128.png'));
const packages = [
  packageOf('demo-data', 'data', data, `demo-data-${version}.nap`),
  packageOf('demo-image', 'resource', image, `demo-image-${version}.nap`),
];
const extensionVersion = JSON.parse(readFileSync(resolve(ROOT, 'extension/manifest.json'), 'utf8')).version;
const catalog = {
  catalogVersion: 2,
  publishedAt: new Date(Number(process.env.SOURCE_DATE_EPOCH || Date.now() / 1000) * 1000).toISOString(),
  apps: [{
    app_id: 'com.natives.app.demo', kind: 'extension_app', name: 'Demo', version,
    minExtensionVersion: extensionVersion, minHostVersion: '0.1.0',
    description: { zh_CN: '静态资源与数据读取示例', en: 'Static resource and data demo' },
    icon: 'grid', permissions: [], runtime_spec: { version },
    surface: { icon: 'grid', route: 'app.html?app=com.natives.app.demo' },
    manifest: { permissions: [] }, packages, published: true,
  }, {
    app_id: 'fund', kind: 'extension_app', name: '基金', version: '0.1.0',
    minExtensionVersion: extensionVersion, minHostVersion: '0.1.0',
    description: { zh_CN: '个人基金资产管理', en: 'Personal fund portfolio' },
    icon: 'box', permissions: ['keychain:com.natives.app.fund'], runtime_spec: { version: '0.1.0' },
    surface: { icon: 'box', route: 'app.html?app=fund' },
    manifest: { permissions: ['keychain:com.natives.app.fund'] }, packages: [], published: false,
  }],
};
writeFileSync(join(outputDir, 'catalog-v2.json'), JSON.stringify(catalog, null, 2) + '\n');
console.log(`generated unsigned app release candidate in ${outputDir}`);
