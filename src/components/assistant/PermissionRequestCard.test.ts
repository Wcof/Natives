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

test('layout: full-width card; outer container owns max-w 860 alignment', () => {
  // Card itself is w-full only — Workbench wraps with MessageInput's max-w-[860px] class.
  assert.match(source, /className="w-full my-3 rounded-xl/);
  assert.equal(source.includes('max-w-[min(860px,100%)]'), false);
  assert.match(source, /data-permission-actions/);
  assert.match(source, /flex flex-col gap-2 w-full/);
  // Each action button is full width (not side-by-side chips).
  assert.match(source, /className=\{`w-full px-3 py-2/);
  // No chip-style scope selector (setScope removed).
  assert.equal(source.includes('setScope'), false);
  assert.equal(source.includes("Scope:"), false);
  assert.equal(source.includes('授权范围'), false);
});

test('button order: once → this_run → project → reject (reject last)', () => {
  const actionsIdx = source.indexOf('data-permission-actions');
  assert.ok(actionsIdx > 0);
  const actionsBlock = source.slice(actionsIdx, source.indexOf('data-permission-error', actionsIdx));
  const onceIdx = actionsBlock.indexOf("PERMISSION_APPROVE_SCOPES.map");
  const rejectIdx = actionsBlock.indexOf('data-permission-reject');
  assert.ok(onceIdx >= 0, 'approve scopes rendered via ordered map');
  assert.ok(rejectIdx > onceIdx, 'reject button after approve scopes');
});

test('details collapsed by default; raw input behind toggle', () => {
  assert.match(source, /useState\(false\)/);
  assert.match(source, /data-permission-details-toggle/);
  assert.match(source, /data-permission-details/);
  assert.match(source, /JSON\.stringify\(request\.input/);
  // Default view shows tool + reason, not raw pre outside toggle.
  const bodyStart = source.indexOf('{/* Body */}');
  const actionsStart = source.indexOf('data-permission-actions');
  const body = source.slice(bodyStart, actionsStart);
  assert.match(body, /data-permission-tool/);
  assert.match(body, /data-permission-reason/);
  // Expanded details only when detailsOpen
  assert.match(body, /detailsOpen \? \(/);
});

test('async handlers: await, lock while submitting, unlock + error on failure', () => {
  assert.match(source, /void \| Promise<void>/);
  assert.match(source, /await action\(\)/);
  assert.match(source, /setSubmitting\(true\)/);
  assert.match(source, /setSubmitting\(false\)/);
  assert.match(source, /data-permission-error/);
  assert.match(source, /role="alert"/);
  assert.match(source, /if \(submitting\) return/);
  assert.match(source, /disabled=\{submitting\}/);
  assert.match(source, /data-submitting=\{submitting \? 'true' : 'false'\}/);
  // Success keeps lock (no setSubmitting(false) in try success path).
  const runAction = source.slice(
    source.indexOf('const runAction = useCallback'),
    source.indexOf('const handleApprove'),
  );
  const tryBlock = runAction.slice(runAction.indexOf('try {'), runAction.indexOf('} catch'));
  assert.equal(tryBlock.includes('setSubmitting(false)'), false);
  assert.match(runAction, /catch \(err\)/);
  assert.match(runAction, /setSubmitting\(false\)/);
});

test('keyboard: Tab-traversable buttons, Enter native activate, Escape rejects, focus-visible', () => {
  // Real <button type="button"> — Tab + Enter work natively.
  assert.match(source, /type="button"/);
  assert.match(source, /event\.key !== 'Escape'/);
  assert.match(source, /handleReject\(\)/);
  assert.match(source, /focus-visible:ring-2/);
  assert.match(source, /focus-visible:outline-none/);
  // Escape documented for screen readers / tests.
  assert.match(source, /escapeHint/);
  assert.match(source, /sr-only/);
});

test('pending-only: non-pending returns null', () => {
  assert.match(source, /if \(request\.status !== 'pending'\)/);
  assert.match(source, /return null/);
});

test('permissionCopy: zh / en labels for all actions', () => {
  const zh = permissionCopy('zh-CN');
  const en = permissionCopy('en-US');
  assert.equal(zh.title, '工具请求授权');
  assert.equal(en.title, 'Tool permission request');
  assert.equal(zh.allowOnce, '仅允许本次');
  assert.equal(en.allowOnce, 'Allow once');
  assert.equal(zh.allowThisRun, '本次运行始终允许');
  assert.equal(en.allowThisRun, 'Always allow this run');
  assert.equal(zh.allowProject, '当前项目始终允许');
  assert.equal(en.allowProject, 'Always allow for this project');
  assert.equal(zh.reject, '拒绝');
  assert.equal(en.reject, 'Reject');
  assert.equal(zh.showDetails, '查看详细参数');
  assert.equal(en.showDetails, 'View request details');
  assert.equal(zh.processing, '处理中…');
  assert.equal(en.processing, 'Processing…');
});

test('labelForScope maps scopes without renaming', () => {
  const copy = permissionCopy('en');
  const scopes: PermissionScope[] = ['once', 'this_run', 'project'];
  assert.equal(labelForScope('once', copy), copy.allowOnce);
  assert.equal(labelForScope('this_run', copy), copy.allowThisRun);
  assert.equal(labelForScope('project', copy), copy.allowProject);
  assert.deepEqual(scopes, ['once', 'this_run', 'project']);
});
