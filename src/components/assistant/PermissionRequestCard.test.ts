import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  labelForScope,
  permissionCopy,
  PERMISSION_APPROVE_SCOPES,
  type PermissionScope,
} from './PermissionRequestCard';

const source = readFileSync(new URL('./PermissionRequestCard.tsx', import.meta.url), 'utf8');

test('approve scopes keep once / this_run / project semantics and display order', () => {
  assert.deepEqual([...PERMISSION_APPROVE_SCOPES], ['once', 'this_run', 'project']);
  // Source must pass scope strings through unchanged — no remapping.
  assert.match(source, /onApprove\(request\.id, scope\)/);
  assert.match(source, /data-permission-scope=\{scope\}/);
  assert.equal(source.includes("'always'"), false);
  assert.equal(source.includes('"session"'), false);
});

test('layout: built on InteractionPromptShell; full-width vertical actions', () => {
  assert.match(source, /from '\.\/InteractionPromptShell'/);
  assert.match(source, /<InteractionPromptShell/);
  // Card body is shell children; actions stay full-width stack.
  assert.match(source, /data-permission-actions/);
  assert.match(source, /flex w-full flex-col gap-2/);
  assert.match(source, /className=\{`w-full rounded-lg/);
  // No chip-style scope selector (setScope removed).
  assert.equal(source.includes('setScope'), false);
  assert.equal(source.includes("Scope:"), false);
  assert.equal(source.includes('授权范围'), false);
});

test('button order: once → this_run → project → reject (reject last)', () => {
  const actionsIdx = source.indexOf('data-permission-actions');
  assert.ok(actionsIdx > 0);
  const actionsBlock = source.slice(actionsIdx);
  const onceIdx = actionsBlock.indexOf('PERMISSION_APPROVE_SCOPES.map');
  const rejectIdx = actionsBlock.indexOf('data-permission-reject');
  assert.ok(onceIdx >= 0, 'approve scopes rendered via ordered map');
  assert.ok(rejectIdx > onceIdx, 'reject button after approve scopes');
});

test('details collapsed by default; raw input behind toggle', () => {
  assert.match(source, /useState\(false\)/);
  assert.match(source, /data-permission-details-toggle/);
  assert.match(source, /data-permission-details/);
  assert.match(source, /JSON\.stringify\(request\.input/);
  assert.match(source, /data-permission-tool/);
  assert.match(source, /data-permission-reason/);
  assert.match(source, /detailsOpen \? \(/);
});

test('async handlers: await, lock while submitting, unlock + error on failure', () => {
  assert.match(source, /void \| Promise<void>/);
  assert.match(source, /await action\(\)/);
  assert.match(source, /setSubmitting\(true\)/);
  assert.match(source, /setSubmitting\(false\)/);
  // Error surfaces via InteractionPromptShell error prop / role=alert in shell.
  assert.match(source, /error=\{error\}/);
  assert.match(source, /if \(submitting\) return/);
  assert.match(source, /disabled=\{submitting\}/);
  assert.match(source, /data-submitting=\{submitting \? 'true' : 'false'\}/);
  const runAction = source.slice(
    source.indexOf('const runAction = useCallback'),
    source.indexOf('const handleApprove'),
  );
  const tryBlock = runAction.slice(runAction.indexOf('try {'), runAction.indexOf('} catch'));
  assert.equal(tryBlock.includes('setSubmitting(false)'), false);
  assert.match(runAction, /catch \(err\)/);
  assert.match(runAction, /setSubmitting\(false\)/);
});

test('keyboard: Escape rejects via shell onEscape; focus-visible ring present', () => {
  assert.match(source, /onEscape=\{handleReject\}/);
  assert.match(source, /focus-visible:ring-2/);
  assert.match(source, /escapeHint/);
});

test('pending-only: non-pending status returns null', () => {
  assert.match(source, /if \(request\.status !== 'pending'\)/);
  assert.match(source, /return null/);
});

test('permissionCopy exposes all UI strings for locale', () => {
  const zh = permissionCopy('zh');
  const en = permissionCopy('en');
  for (const copy of [zh, en]) {
    assert.ok(copy.title);
    assert.ok(copy.allowOnce);
    assert.ok(copy.allowThisRun);
    assert.ok(copy.allowProject);
    assert.ok(copy.reject);
    assert.ok(copy.processing);
    assert.ok(copy.errorFallback);
    assert.ok(copy.escapeHint);
  }
  // Labels map scopes correctly.
  const scopes: PermissionScope[] = ['once', 'this_run', 'project'];
  for (const scope of scopes) {
    assert.ok(labelForScope(scope, zh).length > 0);
    assert.ok(labelForScope(scope, en).length > 0);
  }
});
