import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildCreateRequest,
  canProceedFromScan,
  defaultLocalTitleFromPath,
  deleteLocalConfirmNote,
  localIssueLabel,
  pickPackageManager,
  planSummaryLines,
} from './local-creative';
import type { LaunchPlan, LocalProjectScanResult } from './tauri-adapter';

const baseScan = (over: Partial<LocalProjectScanResult> = {}): LocalProjectScanResult => ({
  projectRoot: '/tmp/demo',
  projectKind: 'html',
  packageManagerChoices: [],
  scripts: [],
  hasNodeModules: true,
  dependenciesMissing: false,
  toolVersions: {},
  risks: [],
  blockers: [],
  extraManifests: [],
  treeSample: [],
  ...over,
});

describe('local-creative helpers', () => {
  it('title from path', () => {
    assert.equal(defaultLocalTitleFromPath('/Users/a/my-app'), 'my-app');
    assert.equal(defaultLocalTitleFromPath('C:\\\\work\\\\x'), 'x');
  });

  it('package manager pick', () => {
    assert.equal(pickPackageManager(baseScan({ packageManager: 'pnpm' })), 'pnpm');
    assert.equal(
      pickPackageManager(baseScan({ packageManagerChoices: ['yarn'] })),
      'yarn',
    );
    assert.equal(
      pickPackageManager(baseScan({ packageManagerChoices: ['npm', 'pnpm'] })),
      undefined,
    );
  });

  it('scan gate', () => {
    assert.equal(canProceedFromScan(null), false);
    assert.equal(canProceedFromScan(baseScan()), true);
    assert.equal(canProceedFromScan(baseScan({ blockers: ['no node'] })), false);
  });

  it('build create request', () => {
    const plan: LaunchPlan = {
      schemaVersion: 1,
      source: 'rule',
      projectKind: 'vite',
      runtime: 'node_dev_server',
      program: 'npm',
      cwdRelative: '.',
      script: 'dev',
      args: [],
      environmentKeys: [],
      port: { mode: 'auto' },
      openPath: '/',
      healthPath: '/',
      startupTimeoutMs: 60000,
      autoOpen: true,
      reason: 'rule',
    };
    const req = buildCreateRequest({
      projectRoot: '/tmp/demo',
      title: '  ',
      launchMode: 'smart',
      launchPlan: plan,
      autoOpen: true,
      packageManager: 'pnpm',
    });
    assert.equal(req.title, 'demo');
    assert.equal(req.launchPlan?.program, 'pnpm');
  });

  it('plan summary and delete note', () => {
    assert.ok(planSummaryLines(null, 'zh')[0]?.includes('尚未'));
    assert.ok(deleteLocalConfirmNote('en').includes('never deleted'));
    assert.equal(localIssueLabel('orphaned_process', 'zh'), '发现残留进程');
  });
});
