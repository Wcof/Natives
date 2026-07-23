/**
 * Creative App pure helpers tests.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  defaultDeleteOptions,
  isActionBusy,
  mergeActionsWithBusy,
  shouldReloadCreativeCatalog,
  sortCreativeApps,
  sourceBadge,
} from './creative-app';
import type { CreativeAppSummary } from './tauri-adapter';

describe('creative-app catalog channel', () => {
  it('accepts creative-app and module', () => {
    assert.equal(shouldReloadCreativeCatalog('creative-app'), true);
    assert.equal(shouldReloadCreativeCatalog('module'), true);
  });
  it('ignores other channels', () => {
    for (const ch of ['notification', 'theme', 'locale', 'env']) {
      assert.equal(shouldReloadCreativeCatalog(ch), false, ch);
    }
  });
});

describe('creative-app actions', () => {
  it('busy disables all actions', () => {
    const a = mergeActionsWithBusy(
      { canOpen: true, canStart: true, canStop: true, canDelete: true, canRetry: true },
      true,
    );
    assert.deepEqual(a, {
      canOpen: false,
      canStart: false,
      canStop: false,
      canDelete: false,
      canRetry: false,
    });
  });
  it('detects transient busy states', () => {
    assert.equal(isActionBusy('installing'), true);
    assert.equal(isActionBusy('running'), false);
  });
});

describe('creative-app sort and badge', () => {
  it('source badge', () => {
    assert.equal(sourceBadge('internal'), 'internal');
    assert.equal(sourceBadge('external_github'), 'github');
  });
  it('sort running first', () => {
    const apps: CreativeAppSummary[] = [
      {
        id: 'a',
        source: 'internal',
        runtime: 'workshop_static',
        title: 'B',
        version: '1',
        state: 'available',
        actions: { canOpen: true, canStart: false, canStop: true, canDelete: true, canRetry: false },
      },
      {
        id: 'b',
        source: 'external_github',
        runtime: 'docker_run',
        title: 'A',
        version: '1',
        state: 'running',
        actions: { canOpen: true, canStart: false, canStop: true, canDelete: true, canRetry: false },
      },
    ];
    const sorted = sortCreativeApps(apps);
    assert.equal(sorted[0]?.id, 'b');
  });
  it('delete defaults keep volumes and images', () => {
    const d = defaultDeleteOptions();
    assert.equal(d.removeVolumes, false);
    assert.equal(d.removeImages, false);
  });
});
