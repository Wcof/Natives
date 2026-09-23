import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(new URL('..', import.meta.url).pathname);
const forbiddenFiles = [
  '.github/workflows/app-release.yml',
  'docs/standards/technical/06-sub-apps.md',
  'extension/app-download.js',
  'extension/catalog-client.js',
  'extension/app-catalog-policy.js',
  'extension/apps/catalog-v3.json',
  'extension/apps/catalog-v3.sig',
  'extension/apps/demo-ui.js',
  'extension/apps/fund-ui.js',
  'extension/app-module-registry.js',
  'extension/demo-ui.test.mjs',
  'scripts/apps/build-suite-seeds.mjs',
  'scripts/apps/package-fund-release.mjs',
  'scripts/apps/publish-release.mjs',
  'crates/native-file-host/src/app_host.rs',
  'crates/native-file-host/src/app_install.rs',
  'crates/native-file-host/src/app_store/install.rs',
  'crates/native-file-host/src/app_store/reconcile.rs',
  'crates/native-file-host/src/app_store/recovery.rs',
  'crates/native-file-host/src/app_store/seed.rs',
  'installers/macos/resources/external-extension.json.in',
  'installers/windows/build-installer.ps1',
  'installers/windows/install.ps1',
  'installers/windows/launch-workbench.ps1',
  'extension/launch-workbench.mjs',
  'scripts/package-windows-test.mjs',
];
for (const file of forbiddenFiles) assert.equal(existsSync(join(root, file)), false, `${file} is retired`);

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

const banned = /src-tauri|tauri-adapter|Next\.js|react-grid-layout|liquid-glass-react|lucide-react|Child WebView|apps:install_(?:begin|chunk|finish|commit|abort)|apps:suite_prepare|Catalog v3|Suite Seed|\.nap\b/;
for (const file of files(join(root, 'docs/standards'))) {
  if (file.endsWith('00-glossary.md')) continue;
  assert.doesNotMatch(readFileSync(file, 'utf8'), banned, `${file} contains retired architecture`);
}
for (const file of [
  'extension/apps.js',
  'crates/native-file-host/src/app_dispatch.rs',
  'crates/native-file-host/src/protocol.rs',
  'crates/native-file-host/src/main.rs',
]) {
  assert.doesNotMatch(readFileSync(join(root, file), 'utf8'), banned, `${file} exposes retired distribution`);
}
for (const dir of ['extension', 'crates', 'installers', 'scripts']) {
  for (const file of files(join(root, dir))) {
    const relative = file.slice(root.length + 1);
    if (!/\.(?:js|mjs|rs|go|c|m|sh|ps1|cmd)$/.test(file)
        || /(?:^|\/)(?:tests?|fixtures)(?:\/|\.|$)/.test(relative)
        || relative === 'scripts/check-standards-current.mjs') continue;
    assert.doesNotMatch(readFileSync(file, 'utf8'), banned, `${relative} contains retired production architecture`);
  }
}
for (const file of ['scripts/installer-package.mjs', 'scripts/package-windows-local.mjs', 'installers/macos/build-pkg.sh']) {
  assert.doesNotMatch(readFileSync(join(root, file), 'utf8'), /update_url|external-extension\.json/, `${file} exposes store distribution`);
}
for (const locale of ['extension/_locales/zh_CN/messages.json', 'extension/_locales/en/messages.json']) {
  assert.doesNotMatch(
    readFileSync(join(root, locale), 'utf8'),
    /扩展包|Extension [Pp]ackage|suitePrepareFailed|随附应用准备/,
    `${locale} contains retired module-distribution copy`,
  );
}
assert.doesNotMatch(
  readFileSync(join(root, 'extension/native-app-client.js'), 'utf8'),
  /return ['"]com\.natives\.file_manager['"]/,
  'built-in module client must not fall back to Core',
);

// ADR-0031 Architectural Gates
const cargoTomls = ['Cargo.toml', 'crates/native-file-host/Cargo.toml', 'crates/app-runtime/Cargo.toml', 'crates/app-runtime-core/Cargo.toml', 'modules/fund/Cargo.toml'];
for (const c of cargoTomls) {
  const content = readFileSync(join(root, c), 'utf8');
  assert.doesNotMatch(content, /name\s*=\s*["']fund-host["']/, `${c} must not declare fund-host binary target`);
  assert.doesNotMatch(content, /Natives-App-Fund/, `${c} must not reference external Natives-App-Fund`);
}

for (const codeFile of ['extension/app.js', 'extension/apps.js', 'scripts/installer-package.mjs']) {
  const content = readFileSync(join(root, codeFile), 'utf8');
  assert.doesNotMatch(content, /appId\s*===?\s*['"]fund['"]/, `${codeFile} must not have fund-specific special case`);
  assert.doesNotMatch(content, /fund-host/, `${codeFile} must not reference fund-host`);
  assert.doesNotMatch(content, /Natives-App-Fund/, `${codeFile} must not reference Natives-App-Fund`);
}

console.log('active standards and production entry points use the current architecture');
