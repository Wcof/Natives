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
const knownCapabilities = new Set(['local-data', 'network.market.read', 'network.nav.read', 'system.notification', 'ai.inference.request', 'system.link.open', 'platform.secret']);
const knownRequirements = new Set(['ai.usage.read', 'ai.quota.read', 'ai.quota.refresh']);
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
  assert.deepEqual(meta.permissions, meta.capabilities, `${entry.appId}: permissions must equal enforced capabilities`);
  if (meta.views !== undefined) {
    assert.ok(Array.isArray(meta.views), `${entry.appId}: views must be an array`);
    for (const view of meta.views) assert.match(view, /^[a-z][a-z0-9-]{0,31}$/, `${entry.appId}: invalid view token ${view}`);
    assert.equal(new Set(meta.views).size, meta.views.length, `${entry.appId}: views must be unique`);
  }
  for (const capability of meta.capabilities) assert.ok(knownCapabilities.has(capability), `${entry.appId}: unknown capability ${capability}`);
  // ADR-0038 fail-closed：声明 platform.secret 必须伴随 capabilityVersion>=4
  // （fund 3→4 的递增纪律）；降版保留声明即篡改。
  if (meta.capabilities.includes('platform.secret')) {
    assert.ok(meta.capabilityVersion >= 4, `${entry.appId}: platform.secret requires capabilityVersion >= 4 (ADR-0038)`);
  }
  for (const requirement of meta.requires || []) assert.ok(knownRequirements.has(requirement), `${entry.appId}: unknown requirement ${requirement}`);
  for (const action of meta.optionalActions || []) assert.ok(knownRequirements.has(action), `${entry.appId}: unknown optional action ${action}`);
  assert.match(entry.factory || '', /^[a-z][a-z0-9_]*::[A-Za-z][A-Za-z0-9_]*::new$/, 'registry factory must be an explicit Rust constructor');
  // entryRoute must point at this appId
  assert.equal(meta.entryRoute, `app.html?app=${entry.appId}`, 'entryRoute must match appId');
  // declared crate must exist
  const crateCargo = join(ROOT, 'modules', entry.appId, 'Cargo.toml');
  assert.ok(existsSync(crateCargo), `module crate missing: modules/${entry.appId}/Cargo.toml`);
  moduleJsonIds.push(meta.appId);
}
moduleJsonIds.sort();

// 3. Runtime registration is generated directly from this registry.
const runtimeBuild = readFileSync(join(ROOT, 'crates/app-runtime/build.rs'), 'utf8');
const runtimeMain = readFileSync(join(ROOT, 'crates/app-runtime/src/main.rs'), 'utf8');
assert.match(runtimeBuild, /modules\/registry\.json/);
assert.match(runtimeBuild, /module_registry\.rs/);
assert.match(runtimeMain, /include!\(concat!\(env!\("OUT_DIR"\), "\/module_registry\.rs"\)\)/);
const runtimeIds = [...registryIds];

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
