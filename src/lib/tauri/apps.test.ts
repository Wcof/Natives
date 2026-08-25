/**
//! tauri/apps trust-boundary 校验测试（APP-02）。
//!
//! 校验器是纯函数，不触发 IPC：`assertNonEmptyAppId` / `validateAppView` /
//! `assertUniqueAppIds` / `parseAppViews`。契约损坏必须抛结构化
//! [`AppContractError`]，严禁静默 fallback 到 undefined。
*/

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  AppContractError,
  assertNonEmptyAppId,
  assertUniqueAppIds,
  parseAppViews,
  parseAppViewOrNull,
  validateAppView,
  type AppView,
} from './apps';

function fixture(over: Partial<AppView> = {}): AppView {
  return {
    appId: 'app-1',
    title: 'Fixtures',
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
      riskLevel: 0,
    },
    runtimeState: 'stopped',
    updatedAt: '2026-08-25T00:00:00Z',
    ...over,
  };
}

function assertThrows(
  fn: () => unknown,
  code: AppContractError['code'],
): AppContractError {
  try {
    fn();
  } catch (e) {
    assert.ok(e instanceof AppContractError, `expected AppContractError, got ${String(e)}`);
    const err = e as AppContractError;
    assert.equal(err.code, code);
    return err;
  }
  throw new assert.AssertionError({ message: `expected AppContractError[${code}], no throw` });
}

describe('apps adapter trust-boundary (APP-02)', () => {
  it('accepts a well-formed AppView', () => {
    const v = validateAppView(fixture(), 'ctx');
    assert.equal(v.appId, 'app-1');
    assert.equal(v.kind, 'web_application');
    assert.equal(v.runtimeState, 'stopped');
    assert.equal(v.updatedAt, '2026-08-25T00:00:00Z');
  });

  it('accepts optional sidebarOrder / description', () => {
    const v = validateAppView(fixture({ sidebarOrder: undefined, description: 'd' }), 'ctx');
    assert.equal(v.sidebarOrder, undefined);
    assert.equal(v.description, 'd');
  });

  it('rejects empty appId (EMPTY_APP_ID), never keeps undefined id', () => {
    for (const empty of ['', '  ', undefined, null, 42]) {
      assertThrows(
        () => validateAppView(fixture({ appId: empty as unknown as string }), 'ctx'),
        'EMPTY_APP_ID',
      );
    }
  });

  it('rejects a non-object payload as MALFORMED_VIEW', () => {
    assertThrows(() => validateAppView(undefined as never, 'ctx'), 'MALFORMED_VIEW');
    assertThrows(() => validateAppView('nope' as never, 'ctx'), 'MALFORMED_VIEW');
  });

  it('assertNonEmptyAppId narrows to string on valid input', () => {
    const id: unknown = 'app-9';
    assertNonEmptyAppId(id, 'ctx');
    assert.equal(id, 'app-9');
    assert.throws(() => assertNonEmptyAppId('', 'ctx'), AppContractError);
  });

  it('parseAppViews enforces unique appIds (DUPLICATE_APP_IDS)', () => {
    const l = parseAppViews([fixture(), fixture({ appId: 'app-2' })], 'ctx');
    assert.equal(l.length, 2);

    const dup = assertThrows(() => parseAppViews([fixture(), fixture()], 'ctx'), 'DUPLICATE_APP_IDS');
    assert.equal(dup.duplicateId, 'app-1');
  });

  it('parseAppViews rejects non-array payload as MALFORMED_VIEW', () => {
    assertThrows(() => parseAppViews({} as never, 'ctx'), 'MALFORMED_VIEW');
    assertThrows(() => parseAppViews(null as never, 'ctx'), 'MALFORMED_VIEW');
  });

  it('parseAppViewOrNull passes null/undefined through', () => {
    assert.equal(parseAppViewOrNull(null, 'ctx'), null);
    assert.equal(parseAppViewOrNull(undefined, 'ctx'), null);
    assert.equal(parseAppViewOrNull(fixture(), 'ctx')?.appId, 'app-1');
  });

  it('validateAppView fails fast on missing appId in a list element', () => {
    const bad = { ...fixture(), appId: undefined };
    assertThrows(() => parseAppViews([fixture(), bad as never], 'ctx'), 'EMPTY_APP_ID');
  });

  it('assertUniqueAppIds surfaces the duplicate which component keys would collide on', () => {
    assertThrows(
      () => assertUniqueAppIds([fixture(), fixture({ appId: 'x' }), fixture()], 'ctx'),
      'DUPLICATE_APP_IDS',
    );
  });
});