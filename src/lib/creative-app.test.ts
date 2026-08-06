/**
 * Creative App pure helpers tests.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  defaultDeleteOptions,
  deleteNeedsDockerOptions,
  deriveBusyIds,
  isActionBusy,
  isOperationActive,
  isOperationTerminal,
  mergeActionsWithBusy,
  openSurfaceKind,
  operationLabelKey,
  operationPhaseLabelKey,
  shouldAutoOpenAfterStart,
  shouldReloadCreativeCatalog,
  sortCreativeApps,
  sourceBadge,
  upsertOperation,
} from './creative-app';
import type { CreativeAppOperation, CreativeAppSummary } from './tauri-adapter';

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
        applicationId: 'app-a',
        source: 'internal',
        runtime: 'workshop_static',
        title: 'B',
        version: '1',
        state: 'available',
        actions: { canOpen: true, canStart: false, canStop: true, canDelete: true, canRetry: false },
      },
      {
        id: 'b',
        applicationId: 'app-b',
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
    applicationId: partial.applicationId ?? `app-${partial.id}`,
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

describe('creative-app operation projection (batch 2 CR-203)', () => {
  function op(partial: Partial<CreativeAppOperation> & Pick<CreativeAppOperation, 'id' | 'phase'>): CreativeAppOperation {
    return {
      kind: 'start',
      actor: 'user',
      startedAt: 't',
      updatedAt: 't',
      ...partial,
    };
  }

  it('classifies active vs terminal phases', () => {
    assert.equal(isOperationActive(op({ id: 1, phase: 'pending' })), true);
    assert.equal(isOperationActive(op({ id: 2, phase: 'waiting' })), true);
    assert.equal(isOperationActive(op({ id: 3, phase: 'running' })), true);
    assert.equal(isOperationActive(op({ id: 4, phase: 'compensating' })), true);
    assert.equal(isOperationActive(op({ id: 5, phase: 'succeeded' })), false);
    assert.equal(isOperationActive(op({ id: 6, phase: 'failed' })), false);
    assert.equal(isOperationActive(op({ id: 7, phase: 'cancelled' })), false);
    assert.equal(isOperationTerminal(op({ id: 8, phase: 'failed' })), true);
    assert.equal(isOperationTerminal(op({ id: 9, phase: 'running' })), false);
  });

  it('upsert merges operations immutably', () => {
    const base = new Map<number, CreativeAppOperation>();
    const a = op({ id: 1, phase: 'running', applicationId: 'app-a' });
    const one = upsertOperation(base, a);
    assert.equal(one.size, 1);
    assert.equal(base.size, 0, 'original map is not mutated');
    const two = upsertOperation(one, op({ id: 2, phase: 'waiting', applicationId: 'app-b' }));
    assert.equal(two.size, 2);
    const updated = upsertOperation(two, op({ id: 1, phase: 'succeeded', applicationId: 'app-a' }));
    assert.equal(updated.size, 2, 'terminal replaces the active entry');
    assert.equal(updated.get(1)?.phase, 'succeeded');
  });

  it('deriveBusyIds maps active operations to source ids', () => {
    const apps = [
      summary({ id: 'l1', source: 'local_project', state: 'running', applicationId: 'app-l1' }),
      summary({ id: 'e1', source: 'external_github', state: 'running', runtime: 'docker_run', applicationId: 'app-e1' }),
    ];
    const operations = [
      op({ id: 1, phase: 'running', applicationId: 'app-l1' }),
      op({ id: 2, phase: 'waiting', applicationId: 'app-e1' }),
      op({ id: 3, phase: 'succeeded', applicationId: 'app-l1' }), // terminal → not busy
      op({ id: 4, phase: 'running', applicationId: 'unknown-app' }), // no matching summary → not busy
    ];
    const busy = deriveBusyIds(apps, operations);
    assert.equal(busy.has('l1'), true);
    assert.equal(busy.has('e1'), true);
    assert.equal(busy.size, 2);
  });

  it('deriveBusyIds ignores operations with no application link', () => {
    const apps = [summary({ id: 'l1', source: 'local_project', state: 'running', applicationId: 'app-l1' })];
    const busy = deriveBusyIds(apps, [op({ id: 1, phase: 'running', applicationId: null })]);
    assert.equal(busy.size, 0);
  });

  it('label keys point into creative.operation', () => {
    assert.equal(operationLabelKey('start'), 'creative.operation.start');
    assert.equal(operationLabelKey('install'), 'creative.operation.install');
    assert.equal(operationPhaseLabelKey('running'), 'creative.operation.running');
    assert.equal(operationPhaseLabelKey('failed'), 'creative.operation.failed');
  });
});
