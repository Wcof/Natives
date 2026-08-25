import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  reduceAppTarget,
  isWebTargetPresented,
  UNLOCAL_PROJECT_ERROR,
  type ActiveAppTarget,
} from './app-target';
import type { AppView } from './tauri/apps';

function view(over: Partial<AppView>): AppView {
  return {
    appId: 'a1',
    title: 'ChatGPT',
    kind: 'web_application',
    registrationOrigin: 'manual',
    showInSidebar: true,
    capabilities: {
      canStart: true,
      canStop: false,
      canRestart: false,
      canOpen: true,
      canEdit: true,
      canRemove: true,
      canSidebar: true,
      riskLevel: 1,
    },
    runtimeState: 'stopped',
    updatedAt: '2026-08-25T00:00:00Z',
    ...over,
  };
}

describe('app-target 状态机（APPV2-T02 Shell/Sidebar 状态转换测试）', () => {
  it('request → switching（含本地项目临时 kind 兜底）', () => {
    const t = reduceAppTarget(null, { type: 'request', appId: 'a1' });
    assert.equal(t?.phase, 'switching');
    assert.equal(t?.view, null);
    assert.equal(t?.kind, 'web_application');

    // 从 error 恢复时保留已解析 view。
    const errored: ActiveAppTarget = { appId: 'a1', kind: 'web_application', view: view({}), phase: 'error', error: 'x' };
    const retry = reduceAppTarget(errored, { type: 'request', appId: 'a1' });
    assert.equal(retry?.phase, 'switching');
    assert.equal(retry?.view?.appId, 'a1');
    assert.equal(retry?.error, null);
  });

  it('viewResolved(web) 保持 switching 并更新 view/kind', () => {
    const t = reduceAppTarget(null, { type: 'request', appId: 'a1' })!;
    const next = reduceAppTarget(t, { type: 'viewResolved', appId: 'a1', view: view({}) })!;
    assert.equal(next.phase, 'switching');
    assert.equal(next.view?.title, 'ChatGPT');
  });

  it('local_project 解析后进入 unsupported error（UI 层切割判定）', () => {
    const t = reduceAppTarget(null, { type: 'request', appId: 'l1' })!;
    const next = reduceAppTarget(t, { type: 'viewResolved', appId: 'l1', view: view({ appId: 'l1', kind: 'local_project' }) })!;
    assert.equal(next.phase, 'error');
    assert.equal(next.error, UNLOCAL_PROJECT_ERROR);
    assert.equal(next.view?.kind, 'local_project');
  });

  it('presented 仅在目标匹配时推进', () => {
    const t = reduceAppTarget(null, { type: 'request', appId: 'a1' })!;
    const ok = reduceAppTarget(t, { type: 'presented', appId: 'a1' })!;
    assert.equal(ok.phase, 'presented');

    // 过期 presented（已切到别的目标）不改变当前状态。
    const other = reduceAppTarget(null, { type: 'request', appId: 'b2' })!;
    const stale = reduceAppTarget(other, { type: 'presented', appId: 'a1' });
    assert.equal(stale, other);
  });

  it('failed 保留可恢复错误；过期 failed 忽略', () => {
    const t = reduceAppTarget(null, { type: 'request', appId: 'a1' })!;
    const failed = reduceAppTarget(t, { type: 'failed', appId: 'a1', error: 'network' })!;
    assert.equal(failed.phase, 'error');
    assert.equal(failed.error, 'network');

    const other = reduceAppTarget(null, { type: 'request', appId: 'b2' })!;
    assert.equal(reduceAppTarget(other, { type: 'failed', appId: 'a1', error: 'x' }), other);
  });

  it('cleared 回到 null；isWebTargetPresented 只认 web 呈现中', () => {
    const t = reduceAppTarget(null, { type: 'request', appId: 'a1' })!;
    const presented = reduceAppTarget(t, { type: 'presented', appId: 'a1' })!;
    assert.equal(isWebTargetPresented(presented), true);
    const mac: ActiveAppTarget = { ...presented, kind: 'system_application' };
    assert.equal(isWebTargetPresented(mac), false);
    assert.equal(reduceAppTarget(presented, { type: 'cleared' }), null);
  });
});
