/**
 * Catalog grouping is the whole point of the ADR-0014 rework, so it is the one
 * piece worth testing without a renderer.
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { groupCreativeApps } from './CreativeCatalog';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

function app(
  partial: Pick<CreativeAppSummary, 'id' | 'source' | 'state'> & Partial<CreativeAppSummary>,
): CreativeAppSummary {
  return {
    applicationId: partial.applicationId ?? `app-${partial.id}`,
    runtime: partial.source === 'internal' ? 'workshop_static' : 'local_static',
    title: partial.id,
    version: '1',
    actions: {
      canOpen: true,
      canStart: false,
      canStop: false,
      canDelete: true,
      canRetry: false,
    },
    ...partial,
  };
}

describe('groupCreativeApps', () => {
  it('splits internal from imported sources', () => {
    const { creations, imported } = groupCreativeApps([
      app({ id: 'gh', source: 'external_github', state: 'installed_stopped' }),
      app({ id: 'in', source: 'internal', state: 'available' }),
      app({ id: 'lp', source: 'local_project', state: 'installed_stopped' }),
    ]);
    assert.deepEqual(creations.map((a) => a.id), ['in']);
    assert.deepEqual(imported.map((a) => a.id).sort(), ['gh', 'lp']);
  });

  it('keeps the shared ordering inside each group', () => {
    const { imported } = groupCreativeApps([
      app({ id: 'stopped', source: 'local_project', state: 'installed_stopped' }),
      app({ id: 'running', source: 'external_github', state: 'running' }),
      app({ id: 'failed', source: 'local_project', state: 'start_failed' }),
    ]);
    assert.deepEqual(imported.map((a) => a.id), ['running', 'stopped', 'failed']);
  });

  it('yields two empty groups for an empty catalog', () => {
    const groups = groupCreativeApps([]);
    assert.deepEqual(groups, { creations: [], imported: [] });
  });

  it('does not mutate the input array', () => {
    const input = [
      app({ id: 'b', source: 'internal', state: 'available' }),
      app({ id: 'a', source: 'internal', state: 'available' }),
    ];
    groupCreativeApps(input);
    assert.deepEqual(input.map((a) => a.id), ['b', 'a']);
  });
});
