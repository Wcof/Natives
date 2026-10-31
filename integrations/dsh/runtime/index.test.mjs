import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  buildEnv,
  checkSchema,
  openModuleUrl,
  productSources,
  resolveCore,
  verifyPayload,
} from './index.js';

const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');

const manifest = {
  modules: [
    { appId: 'fund', dataSchemaVersion: 29 },
    { appId: 'tokenusage', dataSchemaVersion: 3 },
  ],
};

test('schema guard: equal or older passes, newer refuses', () => {
  assert.deepEqual(checkSchema(null, manifest), { ok: true });
  assert.deepEqual(checkSchema({ modules: {} }, manifest), { ok: true });
  assert.deepEqual(checkSchema({ modules: { fund: { dataSchemaVersion: 29 } } }, manifest), { ok: true });
  assert.deepEqual(checkSchema({ modules: { fund: { dataSchemaVersion: 28 } } }, manifest), { ok: true });
  assert.deepEqual(checkSchema({ modules: { fund: { dataSchemaVersion: 30 } } }, manifest), {
    ok: false,
    code: 'schema-too-new',
    appId: 'fund',
    recorded: 30,
    runtime: 29,
  });
  // 未知模块不阻塞：没有可比较的运行时版本。
  assert.deepEqual(checkSchema({ modules: { unknown: { dataSchemaVersion: 99 } } }, manifest), { ok: true });
});

test('core resolution: override, product sources order, missing', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'natives-runtime-'));
  const sourceA = path.join(root, 'a');
  const sourceB = path.join(root, 'b');
  fs.mkdirSync(path.join(sourceA, 'hosts'), { recursive: true });
  fs.mkdirSync(sourceB, { recursive: true });
  fs.writeFileSync(path.join(sourceA, 'hosts', 'native-file-host'), '');
  fs.writeFileSync(path.join(sourceB, 'native-file-host'), '');
  const env = { HOME: root, NATIVES_DEV_PRODUCT_SOURCE: sourceA };

  assert.deepEqual(
    { ...resolveCore({ env, pluginDir: root }), binary: path.basename(resolveCore({ env, pluginDir: root }).binary) },
    { binary: 'native-file-host', source: sourceA, override: false, mode: 'installed' }
  );
  assert.equal(resolveCore({ env: { HOME: root }, sources: [sourceB], pluginDir: root }).source, sourceB);
  assert.equal(resolveCore({ env: { HOME: root }, sources: [path.join(root, 'missing')], pluginDir: root }), null);
  assert.equal(resolveCore({ env: { HOME: root, NATIVES_CORE_BIN: path.join(root, 'nope') }, pluginDir: root }), null);
  assert.ok(productSources({ HOME: root }).at(-1).startsWith('/Library/Application Support/'));
});

test('child env: explicit state dir only, core override never inherited', () => {
  const env = {
    HOME: '/tmp/home',
    NATIVES_STATE_DIR: '/tmp/state',
    NATIVES_CORE_BIN: '/tmp/core',
    PATH: '/usr/bin',
  };
  const child = buildEnv({ env, source: '/tmp/source' });
  assert.equal(child.NATIVES_STATE_DIR, '/tmp/state');
  assert.equal(child.NATIVES_DEV_PRODUCT_SOURCE, '/tmp/source');
  assert.equal('NATIVES_CORE_BIN' in child, false);
  assert.equal(env.NATIVES_CORE_BIN, '/tmp/core');

  // No override: the Core keeps its own default root instead of being pinned.
  const bare = buildEnv({ env: { HOME: '/tmp/home' }, source: '/tmp/source' });
  assert.equal('NATIVES_STATE_DIR' in bare, false);
});

test('url: target whitelist, launcher failures and theme insertion', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'natives-launcher-'));
  const source = path.join(root, 'source');
  fs.mkdirSync(source, { recursive: true });
  const script = path.join(source, 'native-file-host');
  fs.writeFileSync(
    script,
    `#!/bin/sh\nprintf '%s\\n' "{\\"url\\":\\"http://127.0.0.1:8770/$2.html?embed=G&anc=dsh-app%3A+dsh%3A%20http%3A%2F%2F127.0.0.1%3A59737#token=${'a'.repeat(43)}\\"}"\n`,
    { mode: 0o755 }
  );
  const env = { HOME: root, NATIVES_DEV_PRODUCT_SOURCE: source };

  assert.equal((await openModuleUrl({ target: 'files', theme: 'archive', env, pluginDir: root })).includes('files.html'), true);
  const themed = await openModuleUrl({ target: 'files', theme: 'volt', env, pluginDir: root });
  assert.equal(themed.includes('theme=volt'), true);
  // ADR-0048 standalone mode: the DSH host hides product chrome inside the panel.
  assert.equal(themed.includes('standalone=1'), true);
  // The launcher's percent-encoded ancestors must survive theme insertion:
  // a URLSearchParams round-trip would rewrite `%20` into `+` and break the
  // whitespace-separated source list the Core decodes.
  assert.equal(themed.includes('anc=dsh-app%3A+dsh%3A%20http%3A%2F%2F127.0.0.1%3A59737'), true);
  // fragment 保持在末尾，token 未被改写
  assert.equal(themed.endsWith(`#token=${'a'.repeat(43)}`), true);
  assert.equal((await openModuleUrl({ target: 'files', theme: 'nope', env, pluginDir: root })).includes('theme=archive'), true);

  await assert.rejects(openModuleUrl({ target: '../etc/passwd', theme: 'archive', env, pluginDir: root }), {
    code: 'unknown-target',
  });
  await assert.rejects(openModuleUrl({ target: 'space', theme: 'archive', env: { HOME: root }, pluginDir: root }), {
    code: 'core-missing',
  });

  fs.writeFileSync(script, '#!/bin/sh\necho not-json\n', { mode: 0o755 });
  await assert.rejects(openModuleUrl({ target: 'space', theme: 'archive', env, pluginDir: root }), { code: 'launcher-failed' });
});

test('url: recorded schema above the runtime refuses before launching', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'natives-guard-'));
  const source = path.join(root, 'source');
  const apps = path.join(root, 'apps');
  fs.mkdirSync(source, { recursive: true });
  fs.mkdirSync(path.join(root, 'product'), { recursive: true });
  const script = path.join(source, 'native-file-host');
  fs.writeFileSync(script, '#!/bin/sh\necho launched >> "$0.log"\n', { mode: 0o755 });
  fs.writeFileSync(path.join(source, 'product-manifest.json'), JSON.stringify(manifest));
  fs.writeFileSync(
    path.join(root, 'product', 'activation.json'),
    JSON.stringify({ modules: { fund: { dataSchemaVersion: 31 } } })
  );
  const env = { HOME: root, NATIVES_DEV_PRODUCT_SOURCE: source, NATIVES_APPS_ROOT: apps };

  await assert.rejects(openModuleUrl({ target: 'fund', theme: 'archive', env, pluginDir: root }), {
    code: 'schema-too-new',
    appId: 'fund',
    recorded: 31,
    runtime: 29,
  });
  assert.equal(fs.existsSync(`${script}.log`), false, 'must not launch a mismatched Core');
});

test('core resolution: bundled payload first, checkout target is dev mode', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'natives-layout-'));
  const plugin = path.join(root, 'plugin');
  fs.mkdirSync(path.join(plugin, 'payload', 'hosts'), { recursive: true });
  fs.writeFileSync(path.join(plugin, 'payload', 'hosts', 'native-file-host'), '');

  const payload = resolveCore({ env: { HOME: root }, sources: [], pluginDir: plugin });
  assert.equal(payload.mode, 'payload');
  assert.equal(payload.source, path.join(plugin, 'payload'));

  const repo = path.join(root, 'repo');
  const space = path.join(repo, 'integrations/dsh/space');
  fs.mkdirSync(space, { recursive: true });
  fs.writeFileSync(path.join(repo, 'Cargo.toml'), '');
  fs.mkdirSync(path.join(repo, 'target/debug'), { recursive: true });
  fs.writeFileSync(path.join(repo, 'target/debug/native-file-host'), '');
  fs.writeFileSync(path.join(repo, 'target/debug/model-host'), '');

  const dev = resolveCore({ env: { HOME: root }, sources: [], pluginDir: space });
  assert.equal(dev.mode, 'dev');
  assert.equal(dev.binary, path.join(repo, 'target/debug/native-file-host'));
  assert.equal(dev.modelHost, path.join(repo, 'target/debug/model-host'));

  // A packaged plugin outside a checkout and without payload still refuses.
  const loose = path.join(root, 'loose');
  fs.mkdirSync(loose, { recursive: true });
  assert.equal(resolveCore({ env: { HOME: root }, sources: [], pluginDir: loose }), null);
});

test('payload: per-file manifest verified before anything is spawned', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'natives-payload-'));
  const plugin = path.join(root, 'plugin');
  const payload = path.join(plugin, 'payload');
  fs.mkdirSync(path.join(payload, 'hosts'), { recursive: true });
  fs.writeFileSync(path.join(payload, 'product-manifest.json'), '{}');
  fs.writeFileSync(path.join(payload, 'product-manifest.sig'), 'sig');
  fs.writeFileSync(path.join(payload, 'hosts', 'native-file-host'), 'core-bytes');
  fs.writeFileSync(
    path.join(payload, 'payload-manifest.json'),
    JSON.stringify({
      version: '0.1.0',
      files: {
        'product-manifest.json': sha('{}'),
        'product-manifest.sig': sha('sig'),
        'hosts/native-file-host': sha('core-bytes'),
      },
    })
  );

  const good = verifyPayload(payload);
  assert.equal(good.ok, true);
  assert.equal(good.version, '0.1.0');
  assert.equal(good.files, 3);

  // Tampering with one payload file refuses the whole bundle.
  fs.writeFileSync(path.join(payload, 'hosts', 'native-file-host'), 'evil');
  assert.deepEqual(verifyPayload(payload), {
    ok: false,
    code: 'payload-file-mismatch',
    file: 'hosts/native-file-host',
  });

  // A missing payload manifest refuses before launch as well.
  const bare = path.join(root, 'bare');
  fs.mkdirSync(bare, { recursive: true });
  assert.equal(verifyPayload(bare).code, 'payload-manifest-missing');

  // Traversal entries are rejected regardless of the referenced file.
  fs.writeFileSync(
    path.join(payload, 'payload-manifest.json'),
    JSON.stringify({ files: { '../escape': sha('x') } })
  );
  assert.deepEqual(verifyPayload(payload), {
    ok: false,
    code: 'payload-path-escape',
    file: '../escape',
  });

  // openModuleUrl refuses a tampered payload before spawning the Core: restore
  // the valid manifest first so the refusal comes from the tampered file.
  fs.writeFileSync(
    path.join(payload, 'payload-manifest.json'),
    JSON.stringify({
      version: '0.1.0',
      files: {
        'product-manifest.json': sha('{}'),
        'product-manifest.sig': sha('sig'),
        'hosts/native-file-host': sha('core-bytes'),
      },
    })
  );
  const env = { HOME: root };
  await assert.rejects(openModuleUrl({ target: 'space', theme: 'archive', env, pluginDir: plugin }), {
    code: 'payload-file-mismatch',
  });
});
