import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { appFixture, CORE_HOST, ORIGIN } from './native-fixture.mjs';

// 单一产品集成检查（plan §5 P6：旧模块分发测试映射为产品语义反例）。
// 验证：握手协议、空 apps 表仍返回固定内置模块（fund）、旧分发方法
// 在任何存储/注册变更前明确拒绝（APP_RETIRED_METHOD）、清数据仍需确认。
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const extensionFiles = ['extension/app.js', 'extension/apps.js', 'extension/manifest.json'];
const extensionDigest = () => digest(Buffer.concat(extensionFiles.map((path) => readFileSync(path))));
const extensionHashBefore = extensionDigest();

const fixture = appFixture();
try {
  const core = fixture.connect(CORE_HOST);
  const host = await core.call('apps:handshake', { origin: ORIGIN });
  assert.equal(host.appsProtocolVersion, 4);

  // 空 apps 表也必须返回产品声明的固定内置模块（plan §3.4）。
  const listed = await core.call('apps:list', {});
  const moduleIds = (listed.modules ?? []).map((m) => m.app_id ?? m.appId);
  assert.ok(
    moduleIds.includes('fund'),
    `fixed module fund must be listed even with empty apps table, got: ${JSON.stringify(moduleIds)}`,
  );
  // 产品投影存在；无远端发现字段。
  assert.ok(listed.product, 'product projection must be present');
  assert.equal(listed.availablePackages, undefined);
  assert.equal(listed.catalog, undefined);

  // 旧模块分发方法：在任何存储/网络/注册变更前明确拒绝（plan §3.4）。
  const retired = [
    'apps:suite_prepare',
    'apps:install_begin',
    'apps:install_chunk',
    'apps:install_finish',
    'apps:install_commit',
    'apps:install_abort',
    'apps:uninstall',
    'apps:rollback',
  ];
  for (const method of retired) {
    await assert.rejects(
      core.call(method, {}),
      /APP_RETIRED_METHOD/,
      `${method} must be rejected as retired`,
    );
  }

  // 清数据仍需确认（数据保护约束继续有效）。
  await assert.rejects(
    core.call('apps:clear_data', { appId: 'fund', requestId: 'integration-clear-1' }),
    /APP_CONFIRMATION_REQUIRED/,
  );

  assert.equal(extensionDigest(), extensionHashBefore);
  console.log(JSON.stringify({
    coreProtocol: 4,
    fixedModules: true,
    retiredMethodsRejected: retired.length,
    clearDataConfirmation: 'passed',
    fixedExtensionHash: extensionHashBefore,
  }));
} finally {
  await fixture.dispose();
}
