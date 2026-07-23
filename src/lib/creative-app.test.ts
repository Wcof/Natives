/**
 * Creative App pure helpers tests.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  defaultDeleteOptions,
  deleteNeedsDockerOptions,
  isActionBusy,
  mergeActionsWithBusy,
  openSurfaceKind,
  shouldAutoOpenAfterStart,
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
    assert.equal(sourceBadge('local_project'), 'local');
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

function summary(
  partial: Partial<CreativeAppSummary> & Pick<CreativeAppSummary, 'id' | 'source' | 'state'>,
): CreativeAppSummary {
  return {
    runtime: partial.source === 'internal' ? 'workshop_static' : 'local_static',
    title: partial.id,
    version: '1',
    actions: {
      canOpen: false,
      canStart: false,
      canStop: false,
      canDelete: true,
      canRetry: false,
    },
    ...partial,
  };
}

describe('creative-app lifecycle matrix helpers', () => {
  it('auto-open only for local projects with flag', () => {
    assert.equal(
      shouldAutoOpenAfterStart(
        summary({
          id: 'l1',
          source: 'local_project',
          state: 'running',
          localProject: {
            projectRoot: '/tmp/x',
            projectKind: 'html',
            launchMode: 'smart',
            deviceId: 'd',
            deviceName: 'n',
            autoOpen: true,
          },
        }),
      ),
      true,
    );
    assert.equal(
      shouldAutoOpenAfterStart(
        summary({
          id: 'l2',
          source: 'local_project',
          state: 'running',
          localProject: {
            projectRoot: '/tmp/x',
            projectKind: 'html',
            launchMode: 'smart',
            deviceId: 'd',
            deviceName: 'n',
            autoOpen: false,
          },
        }),
      ),
      false,
    );
    assert.equal(
      shouldAutoOpenAfterStart(
        summary({ id: 'e1', source: 'external_github', state: 'running', runtime: 'docker_run' }),
      ),
      false,
    );
    assert.equal(
      shouldAutoOpenAfterStart(summary({ id: 'i1', source: 'internal', state: 'available' })),
      false,
    );
  });

  it('open surface and delete options by source', () => {
    assert.equal(
      openSurfaceKind(summary({ id: 'i', source: 'internal', state: 'available' })),
      'workshop',
    );
    assert.equal(
      openSurfaceKind(
        summary({ id: 'e', source: 'external_github', state: 'running', runtime: 'docker_run' }),
      ),
      'local_url',
    );
    assert.equal(
      openSurfaceKind(summary({ id: 'l', source: 'local_project', state: 'running' })),
      'local_url',
    );
    assert.equal(deleteNeedsDockerOptions('external_github'), true);
    assert.equal(deleteNeedsDockerOptions('local_project'), false);
    assert.equal(deleteNeedsDockerOptions('internal'), false);
  });
});
