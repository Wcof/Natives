import assert from 'node:assert/strict';
import test from 'node:test';
import { canSendAssistantDraft, fileNameFromPath, normalizePermissionProfile } from './assistant-composer';

test('allows text or attachment-only drafts', () => {
  assert.equal(canSendAssistantDraft('', []), false);
  assert.equal(canSendAssistantDraft(' hello ', []), true);
  assert.equal(canSendAssistantDraft('', [{ path: '/tmp/a.txt', name: 'a.txt', mimeType: 'text/plain', size: 0 }]), true);
});

test('normalizes permission profiles and file names', () => {
  assert.equal(normalizePermissionProfile('full_access'), 'full_access');
  assert.equal(normalizePermissionProfile('unknown'), 'ask');
  assert.equal(fileNameFromPath('C:\\tmp\\note.md'), 'note.md');
});
