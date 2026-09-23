// Registry Equality Gate (ADR-0031 §8 / plan §45.3):
//   modules/registry.json == Runtime registered IDs == Product Manifest IDs == module directories
// Any mismatch fails CI. Run: node scripts/apps/check-registry-equality.mjs
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

// 1. Registry file
const registry = JSON.parse(readFileSync(join(ROOT, 'modules/registry.json'), 'utf8'));
assert.equal(registry.schemaVersion, 1, 'registry schemaVersion must be 1');
const registryIds = registry.apps.map((a) => a.appId).sort();

// 2. module.json per entry (registry <-> module directory authority)
const moduleJsonIds = [];
for (const entry of registry.apps) {
  const metaPath = join(ROOT, entry.manifest);
  assert.ok(existsSync(metaPath), `missing module manifest: ${entry.manifest}`);
  const meta = JSON.parse(readFileSync(metaPath, 'utf8'));
  assert.equal(meta.appId, entry.appId, `registry appId != module.json appId (${entry.appId})`);
  assert.ok(meta.displayName, 'module.json displayName required');
  assert.ok(meta.entryRoute, 'module.json entryRoute required');
  assert.equal(typeof meta.moduleApiVersion, 'number');
  assert.equal(typeof meta.dataSchemaVersion, 'number');
  assert.equal(typeof meta.capabilityVersion, 'number');
  // entryRoute must point at this appId
  assert.equal(meta.entryRoute, `app.html?app=${entry.appId}`, 'entryRoute must match appId');
  // declared crate must exist
  const crateCargo = join(ROOT, 'modules', entry.appId, 'Cargo.toml');
  assert.ok(existsSync(crateCargo), `module crate missing: modules/${entry.appId}/Cargo.toml`);
  moduleJsonIds.push(meta.appId);
}
moduleJsonIds.sort();

// 3. Runtime registered IDs (compiled into natives-app-runtime via register_builtin_modules)
// 仅统计非 cfg(feature) 门控的注册（test-fixture 第二模块不属于 Release Registry，计划 §44）：
// 剥除 #[cfg(feature = "test-fixture")] 属性行及其紧随的一行，再整体匹配多行 register 调用。
const mainRsLines = readFileSync(join(ROOT, 'crates/app-runtime/src/main.rs'), 'utf8').split('\n');
const releaseLines = [];
for (let i = 0; i < mainRsLines.length; i++) {
  if (/^\s*#\[cfg\(feature\s*=\s*"test-fixture"\)\]/.test(mainRsLines[i])) {
    i++; // 跳过属性紧随的一行（被门控的语句）
    continue;
  }
  releaseLines.push(mainRsLines[i]);
}
const runtimeIds = [...releaseLines.join('\n').matchAll(/registry\.register\(\s*"([a-z0-9][a-z0-9._-]*)"/g)]
  .map((m) => m[1])
  .sort();
assert.ok(runtimeIds.length > 0, 'app-runtime registers no modules');

// 4. Module directories under modules/
const moduleDirs = readdirSync(join(ROOT, 'modules'), { withFileTypes: true })
  .filter((d) => d.isDirectory() && existsSync(join(ROOT, 'modules', d.name, 'module.json')))
  .map((d) => d.name)
  .sort();

// Equality: all four sets identical
assert.deepEqual(registryIds, moduleJsonIds, 'registry.json != module.json set');
assert.deepEqual(registryIds, runtimeIds, `registry.json != runtime registered set\nregistry: ${registryIds}\nruntime:  ${runtimeIds}`);
assert.deepEqual(registryIds, moduleDirs, `registry.json != module directories\nregistry: ${registryIds}\ndirs:     ${moduleDirs}`);

// 5. No Fund special case outside modules/fund / fund tests (plan §8.5)
import { statSync } from 'node:fs';
const offenders = [];
for (const rel of ['crates/native-file-host/src', 'crates/app-runtime/src', 'crates/app-runtime-core/src', 'scripts/installer-package.mjs', 'extension']) {
  const path = join(ROOT, rel);
  const files = statSync(path).isDirectory()
    ? readdirSync(path).map((name) => join(path, name)).filter((f) => /\.(rs|mjs|js)$/.test(f) && statSync(f).isFile())
    : [path];
  for (const f of files) {
    const text = readFileSync(f, 'utf8');
    if (/if\s+app_id\s*==\s*"fund"|FIXED_MODULES\s*=\s*&\[FixedModule\s*\{\s*app_id:\s*"fund"/.test(text)) {
      offenders.push(rel);
    }
  }
}
assert.deepEqual(offenders, [], `Fund special case found outside modules/fund: ${offenders.join(', ')}`);

console.log(`registry equality OK: ${registryIds.join(', ')} (registry == runtime == manifest source == dirs)`);
