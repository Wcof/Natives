/**
 * ExecutionEngineSettingsPanel — SETTINGS-001/002 UI double-layer.
 *
 * The blocked/degraded/disabled/not_installed radios must be disabled (only a
 * `ready` runtime is a selectable default), and the capability table must be
 * projected from the real runtime descriptors — never a hardcoded feature
 * list. The Host save gate independently rejects crafted saves; these tests
 * pin the UI half of that double-layer contract.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { isSelectable } from './ExecutionEngineSettingsPanel';
import type { RuntimeDescriptor } from '@/lib/tauri-adapter';

const panelSource = readFileSync(new URL('./ExecutionEngineSettingsPanel.tsx', import.meta.url), 'utf8');

function descriptor(status: string): RuntimeDescriptor {
  return {
    id: status,
    displayName: status,
    status,
    version: null,
    authority: 'external_bridge',
    reasonCode: `${status}_code`,
    reason: 'test',
    capabilities: {},
    controllable: [],
  };
}

describe('isSelectable (UI gate for default runtime)', () => {
  it('only ready runtimes are selectable as the default', () => {
    assert.equal(isSelectable(descriptor('ready')), true);
  });

  it('blocked/degraded/disabled/not_installed runtimes are not selectable', () => {
    for (const status of ['blocked', 'degraded', 'disabled', 'not_installed']) {
      assert.equal(isSelectable(descriptor(status)), false, `${status} must be non-selectable`);
    }
  });

  it('unknown statuses are treated as non-selectable (fail-closed)', () => {
    assert.equal(isSelectable(descriptor('unknown_status')), false);
  });
});

describe('panel source enforces the double-layer contract', () => {
  it('radio disabled includes the isSelectable gate', () => {
    assert.match(panelSource, /disabled=\{busy \|\| !isSelectable\(rt\)\}/);
  });

  it('capability table is driven by runtime descriptors, not a hardcoded list', () => {
    // The old static self-announcement (fixed cap names x fixed runtime ids)
    // must be gone.
    assert.doesNotMatch(panelSource, /'streaming'.*'tools'.*'mcp'.*'hooks'/s);
    assert.doesNotMatch(panelSource, /\['native', 'claude_cli', 'codex_cli'\]\.map/);
    // Rows come from the union of descriptor capability keys.
    assert.match(panelSource, /capabilityKeys\.map/);
    // Columns come from snapshot.runtimes, not a hardcoded three.
    assert.match(panelSource, /\{snapshot\.runtimes\.map\(\(rt\) =>/);
  });
});
