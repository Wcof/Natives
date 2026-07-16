import assert from 'node:assert/strict';
import test from 'node:test';
import { assistantRetryPrompt, canSendAssistantDraft, fileNameFromPath, mimeTypeFromPath, normalizePermissionProfile } from './assistant-composer';

test('allows text or attachment-only drafts', () => {
  assert.equal(canSendAssistantDraft('', []), false);
  assert.equal(canSendAssistantDraft(' hello ', []), true);
  assert.equal(canSendAssistantDraft('', [{ path: '/tmp/a.txt', name: 'a.txt', mimeType: 'text/plain', size: 0 }]), true);
});

test('rebuilds retry prompts with attachment references', () => {
  assert.equal(assistantRetryPrompt([
    { type: 'text', content: { text: 'Inspect' } },
    { type: 'file_reference', content: { path: '/tmp/a.txt' } },
  ]), 'Inspect\n[Attached file: /tmp/a.txt]');
});

test('normalizes permission profiles and file names', () => {
  assert.equal(normalizePermissionProfile('full_access'), 'full_access');
  assert.equal(normalizePermissionProfile('unknown'), 'ask');
  assert.equal(fileNameFromPath('C:\\tmp\\note.md'), 'note.md');
  assert.equal(mimeTypeFromPath('/tmp/note.md'), 'text/markdown');
});
